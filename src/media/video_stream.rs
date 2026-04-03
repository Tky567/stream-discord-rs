/// Video media stream — ported from `VideoStream.ts`.
///
/// Drains `MediaPacket` frames from a channel and forwards them to
/// `WebRtcWrapper::send_video_frame` with proper PTS-based timing.

use crate::media::base_stream::{BaseMediaStream, MediaPacket, StreamSyncState};
use crate::voice::webrtc::WebRtcWrapper;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::debug;

pub struct VideoStream {
    base: BaseMediaStream,
    webrtc: Arc<tokio::sync::Mutex<WebRtcWrapper>>,
}

impl VideoStream {
    pub fn new(
        webrtc: Arc<tokio::sync::Mutex<WebRtcWrapper>>,
        no_sleep: bool,
    ) -> (Self, Arc<Mutex<StreamSyncState>>) {
        let (base, state) = BaseMediaStream::new(no_sleep);
        (Self { base, webrtc }, state)
    }

    pub fn base_mut(&mut self) -> &mut BaseMediaStream {
        &mut self.base
    }

    /// Drive the stream from an async receiver.  Returns when the channel closes
    /// or `stop_rx` fires.
    pub async fn run(
        &mut self,
        mut rx: mpsc::Receiver<MediaPacket>,
        mut stop_rx: tokio::sync::oneshot::Receiver<()>,
    ) {
        let mut start_time: Option<Instant> = None;
        let mut start_pts: Option<f64> = None;

        loop {
            tokio::select! {
                biased;
                _ = &mut stop_rx => {
                    debug!("VideoStream stopped");
                    break;
                }
                pkt = rx.recv() => {
                    match pkt {
                        None => break,
                        Some(packet) => {
                            let webrtc = self.webrtc.clone();
                            let frametime = packet.frametime_ms();
                            self.base
                                .process_packet(
                                    &packet,
                                    &mut start_time,
                                    &mut start_pts,
                                    |data, ft| async move {
                                        let mut w = webrtc.lock().await;
                                        let _ = w.send_video_frame(&data, ft).await;
                                    },
                                )
                                .await;
                            let _ = frametime;
                        }
                    }
                }
            }
        }

        self.base.mark_ended();
    }
}
