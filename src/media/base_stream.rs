/// Base media stream — ported from `BaseMediaStream.ts`.
///
/// Implements the PTS-based timing / sync logic for draining encoded frames
/// into the Discord WebRTC pipeline at the correct wall-clock rate.

use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Packet (mirrors `node-av` Packet)
// ---------------------------------------------------------------------------

/// A decoded/encoded media frame packet, analogous to the `Packet` type from
/// `node-av`.
#[derive(Debug, Clone)]
pub struct MediaPacket {
    pub data: Vec<u8>,
    /// Presentation timestamp (raw codec time units).
    pub pts: i64,
    /// Frame duration (raw codec time units).
    pub duration: i64,
    /// Timebase: `(num, den)`.  Frametime = `duration * num / den * 1000` ms.
    pub time_base_num: i32,
    pub time_base_den: i32,
}

impl MediaPacket {
    /// Compute the frame duration in milliseconds.
    pub fn frametime_ms(&self) -> f64 {
        (self.duration as f64 / self.time_base_den as f64) * self.time_base_num as f64 * 1000.0
    }

    /// Compute PTS in milliseconds.
    pub fn pts_ms(&self) -> f64 {
        (self.pts as f64 / self.time_base_den as f64) * self.time_base_num as f64 * 1000.0
    }
}

// ---------------------------------------------------------------------------
// Sync state shared between two streams
// ---------------------------------------------------------------------------

/// Mutable PTS state shared via `Arc<Mutex<>>` across audio and video streams
/// so one can check the other's progress.
#[derive(Debug, Default)]
pub struct StreamSyncState {
    pub pts_ms: Option<f64>,
    pub ended: bool,
}

// ---------------------------------------------------------------------------
// BaseMediaStream
// ---------------------------------------------------------------------------

/// Tolerance in ms within which two streams are considered in sync.
const SYNC_TOLERANCE_MS: f64 = 20.0;

pub struct BaseMediaStream {
    pub sync: bool,
    pub no_sleep: bool,
    sync_tolerance: f64,

    /// Our own PTS/ended state (shared with peer stream).
    state: Arc<Mutex<StreamSyncState>>,
    /// The peer stream's state (for A/V sync decisions).
    peer_state: Option<Arc<Mutex<StreamSyncState>>>,
}

impl BaseMediaStream {
    pub fn new(no_sleep: bool) -> (Self, Arc<Mutex<StreamSyncState>>) {
        let state = Arc::new(Mutex::new(StreamSyncState::default()));
        let stream = Self {
            sync: true,
            no_sleep,
            sync_tolerance: SYNC_TOLERANCE_MS,
            state: state.clone(),
            peer_state: None,
        };
        (stream, state)
    }

    /// Link this stream to a peer (e.g. video ↔ audio) for AV sync.
    pub fn set_sync_stream(&mut self, peer: Arc<Mutex<StreamSyncState>>) {
        // Guard against mutual sync
        assert!(
            !Arc::ptr_eq(&self.state, &peer),
            "Cannot sync a stream with itself"
        );
        self.peer_state = Some(peer);
    }

    pub fn sync_tolerance(&self) -> f64 {
        self.sync_tolerance
    }

    pub fn set_sync_tolerance(&mut self, ms: f64) {
        if ms >= 0.0 {
            self.sync_tolerance = ms;
        }
    }

    fn pts_delta(&self) -> Option<f64> {
        let our_pts = self.state.lock().ok()?.pts_ms?;
        let peer_pts = self.peer_state.as_ref()?.lock().ok()?.pts_ms?;
        Some(our_pts - peer_pts)
    }

    fn peer_active(&self) -> bool {
        self.peer_state
            .as_ref()
            .and_then(|p| p.lock().ok())
            .map(|s| !s.ended)
            .unwrap_or(false)
    }

    fn is_ahead(&self) -> bool {
        self.peer_active()
            && self
                .pts_delta()
                .map(|d| d > self.sync_tolerance)
                .unwrap_or(false)
    }

    fn is_behind(&self) -> bool {
        self.peer_active()
            && self
                .pts_delta()
                .map(|d| d < -self.sync_tolerance)
                .unwrap_or(false)
    }

    /// Update our PTS (in ms) after sending a frame.
    pub fn update_pts(&self, pts_ms: f64) {
        if let Ok(mut s) = self.state.lock() {
            s.pts_ms = Some(pts_ms);
        }
    }

    pub fn mark_ended(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.ended = true;
        }
    }

    /// Process one packet: call `send_frame`, then sleep as needed to maintain
    /// wall-clock timing.  Mirrors `_write()` in `BaseMediaStream.ts`.
    pub async fn process_packet<F, Fut>(
        &mut self,
        packet: &MediaPacket,
        start_time: &mut Option<Instant>,
        start_pts: &mut Option<f64>,
        send_frame: F,
    ) where
        F: FnOnce(Vec<u8>, f64) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let frametime = packet.frametime_ms();
        let pts_ms = packet.pts_ms();

        let t0 = Instant::now();
        send_frame(packet.data.clone(), frametime).await;
        let send_dur = t0.elapsed().as_secs_f64() * 1000.0;

        self.update_pts(pts_ms);

        let ratio = send_dur / frametime;
        debug!(
            pts = pts_ms,
            frame_size = packet.data.len(),
            send_ms = send_dur,
            frametime,
            "Frame sent ({:.1}% of frametime)",
            ratio * 100.0
        );
        if ratio > 1.0 {
            warn!(
                frame_size = packet.data.len(),
                send_ms = send_dur,
                frametime,
                "Frame takes too long to send ({:.1}% of frametime)",
                ratio * 100.0
            );
        }

        *start_time = start_time.or(Some(t0));
        *start_pts = start_pts.or(Some(pts_ms));

        let elapsed_ms = start_time.map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0);
        let sleep_ms = (pts_ms - start_pts.unwrap_or(pts_ms) + frametime - elapsed_ms).max(0.0);

        if self.no_sleep || sleep_ms == 0.0 {
            // no sleep
        } else if self.sync && self.is_behind() {
            debug!("Stream is behind; skipping sleep for this frame");
            *start_time = None;
            *start_pts = None;
        } else if self.sync && self.is_ahead() {
            loop {
                debug!("Stream is ahead; waiting {}ms", frametime);
                sleep(Duration::from_secs_f64(frametime / 1000.0)).await;
                if !self.is_ahead() {
                    break;
                }
            }
            *start_time = None;
            *start_pts = None;
        } else {
            sleep(Duration::from_secs_f64(sleep_ms / 1000.0)).await;
        }
    }
}
