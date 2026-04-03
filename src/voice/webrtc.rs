/// WebRTC wrapper — ported from `WebRtcWrapper.ts`.
///
/// Responsibilities:
/// - Manage an `RTCPeerConnection` (ICE + DTLS-SRTP via `webrtc` crate)
/// - Track RTP sequence numbers and timestamps for audio + video
/// - Perform H264 SPS/VUI rewriting before packetization
/// - Apply DAVE E2EE encryption when the session is ready
/// - Send encoded RTP frames to Discord

use crate::dave::DaveHandler;
use crate::processing::{rewrite_sps_vui, split_nalu, AnnexBHelpers, H264Helpers, H264NalUnitType};
use crate::utils::{normalize_video_codec, VideoCodec};
use crate::voice::codec_payload::{self, ALL_VIDEO_CODECS};
use crate::voice::connection::WebRtcParams;
use bytes::Bytes;
use davey::{Codec, MediaType};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::debug;
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS, MIME_TYPE_VP8, MIME_TYPE_VP9},
        APIBuilder,
    },
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration,
        peer_connection_state::RTCPeerConnectionState,
        RTCPeerConnection,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
    track::track_local::{
        track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter,
    },
};

#[derive(Debug, Error)]
pub enum WebRtcError {
    #[error("WebRTC not initialized")]
    NotInitialized,
    #[error("Packetizer not configured")]
    PacketizerNotConfigured,
    #[error("Unknown video codec: {0}")]
    UnknownCodec(String),
    #[error("WebRTC error: {0}")]
    Webrtc(String),
    #[error("DAVE error: {0}")]
    Dave(String),
}

// ---------------------------------------------------------------------------
// RTP state — tracks per-track sequence + timestamp
// ---------------------------------------------------------------------------

struct RtpState {
    seq: u16,
    timestamp: u32,
    ssrc: u32,
    payload_type: u8,
    clock_rate: u32,
}

impl RtpState {
    fn new(ssrc: u32, payload_type: u8, clock_rate: u32) -> Self {
        Self {
            seq: rand_seq(),
            timestamp: rand_timestamp(),
            ssrc,
            payload_type,
            clock_rate,
        }
    }

    /// Build a minimal RTP header + payload bytes ready to ship.
    fn make_packet(&mut self, payload: &[u8], marker: bool) -> Vec<u8> {
        // 12-byte fixed RTP header
        let mut pkt = Vec::with_capacity(12 + payload.len());
        // V=2, P=0, X=0, CC=0
        pkt.push(0x80);
        // M bit + payload type
        pkt.push(if marker { 0x80 | self.payload_type } else { self.payload_type });
        // Sequence number
        pkt.extend_from_slice(&self.seq.to_be_bytes());
        // Timestamp
        pkt.extend_from_slice(&self.timestamp.to_be_bytes());
        // SSRC
        pkt.extend_from_slice(&self.ssrc.to_be_bytes());
        pkt.extend_from_slice(payload);
        self.seq = self.seq.wrapping_add(1);
        pkt
    }

    fn advance_timestamp(&mut self, frametime_ms: f64) {
        self.timestamp = self.timestamp.wrapping_add(
            (frametime_ms * self.clock_rate as f64 / 1000.0).round() as u32,
        );
    }
}

fn rand_seq() -> u16 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        & 0xFFFF) as u16
}

fn rand_timestamp() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

// ---------------------------------------------------------------------------
// WebRtcWrapper
// ---------------------------------------------------------------------------

pub struct WebRtcWrapper {
    peer_connection: Option<Arc<RTCPeerConnection>>,
    audio_track: Option<Arc<TrackLocalStaticRTP>>,
    video_track: Option<Arc<TrackLocalStaticRTP>>,

    audio_state: Option<RtpState>,
    video_state: Option<RtpState>,
    video_codec: Option<VideoCodec>,

    /// Shared handle to the DAVE session for E2EE encryption.
    dave: Arc<Mutex<DaveHandler>>,
}

impl WebRtcWrapper {
    pub fn new(dave: Arc<Mutex<DaveHandler>>) -> Self {
        Self {
            peer_connection: None,
            audio_track: None,
            video_track: None,
            audio_state: None,
            video_state: None,
            video_codec: None,
            dave,
        }
    }

    /// Initialize the WebRTC peer connection and register audio + video tracks.
    /// Mirrors `initWebRtc()` in `WebRtcWrapper.ts`.
    pub async fn init(&mut self) -> Result<Arc<RTCPeerConnection>, WebRtcError> {
        let mut media_engine = MediaEngine::default();

        // Register Opus
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: MIME_TYPE_OPUS.to_owned(),
                        clock_rate: codec_payload::OPUS.clock_rate,
                        channels: 2,
                        sdp_fmtp_line: "minptime=10;useinbandfec=1;usedtx=1".to_owned(),
                        rtcp_feedback: vec![],
                    },
                    payload_type: codec_payload::OPUS.payload_type,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;

        // Register all video codecs
        for vc in ALL_VIDEO_CODECS {
            let mime_type = match vc.name {
                "H264" => MIME_TYPE_H264,
                "H265" => "video/H265",
                "VP8" => MIME_TYPE_VP8,
                "VP9" => MIME_TYPE_VP9,
                "AV1" => "video/AV1",
                _ => continue,
            };
            media_engine
                .register_codec(
                    RTCRtpCodecParameters {
                        capability: RTCRtpCodecCapability {
                            mime_type: mime_type.to_owned(),
                            clock_rate: vc.clock_rate,
                            channels: 0,
                            sdp_fmtp_line: String::new(),
                            rtcp_feedback: vec![],
                        },
                        payload_type: vc.payload_type,
                        ..Default::default()
                    },
                    RTPCodecType::Video,
                )
                .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;
        }

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;

        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let pc = Arc::new(
            api.new_peer_connection(config)
                .await
                .map_err(|e| WebRtcError::Webrtc(e.to_string()))?,
        );

        // Add audio track
        let audio_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "discord-stream-rs".to_owned(),
        ));
        pc.add_track(audio_track.clone())
            .await
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;

        // Add video track (use H264 as default capability)
        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_owned(),
                ..Default::default()
            },
            "video".to_owned(),
            "discord-stream-rs".to_owned(),
        ));
        pc.add_track(video_track.clone())
            .await
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;

        self.audio_track = Some(audio_track);
        self.video_track = Some(video_track);
        self.peer_connection = Some(pc.clone());

        debug!("WebRTC peer connection initialized");
        Ok(pc)
    }

    /// Configure RTP state (SSRC, payload type, clock rate) from the READY
    /// params received from the Discord voice gateway.
    /// Mirrors `setPacketizer()` in `WebRtcWrapper.ts`.
    pub fn set_packetizer(
        &mut self,
        params: &WebRtcParams,
        video_codec: &str,
    ) -> Result<(), WebRtcError> {
        let codec = normalize_video_codec(video_codec)
            .map_err(|e| WebRtcError::UnknownCodec(e.0))?;

        let video_info = match codec {
            VideoCodec::H264 => codec_payload::H264,
            VideoCodec::H265 => codec_payload::H265,
            VideoCodec::Vp8 => codec_payload::VP8,
            VideoCodec::Vp9 => codec_payload::VP9,
            VideoCodec::Av1 => codec_payload::AV1,
        };

        self.audio_state = Some(RtpState::new(
            params.audio_ssrc,
            codec_payload::OPUS.payload_type,
            codec_payload::OPUS.clock_rate,
        ));
        self.video_state = Some(RtpState::new(
            params.video_ssrc,
            video_info.payload_type,
            video_info.clock_rate,
        ));
        self.video_codec = Some(codec);

        debug!("RTP packetizer configured: audio_ssrc={} video_ssrc={} codec={:?}",
            params.audio_ssrc, params.video_ssrc, codec);
        Ok(())
    }

    /// Send an Opus audio frame.
    /// Mirrors `sendAudioFrame()` in `WebRtcWrapper.ts`.
    pub async fn send_audio_frame(
        &mut self,
        frame: &[u8],
        frametime_ms: f64,
    ) -> Result<(), WebRtcError> {
        let track = self.audio_track.as_ref().ok_or(WebRtcError::NotInitialized)?;
        let state = self.audio_state.as_mut().ok_or(WebRtcError::PacketizerNotConfigured)?;

        // DAVE E2EE encrypt before sending
        let encrypted = {
            let mut dave = self.dave.lock().await;
            dave.encrypt_opus(frame).map_err(|e| WebRtcError::Dave(e.to_string()))?
        };

        let pkt = state.make_packet(&encrypted, true);
        state.advance_timestamp(frametime_ms);

        track
            .write(&Bytes::from(pkt))
            .await
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;
        Ok(())
    }

    /// Send a video frame (any codec).
    /// For H264, rewrites SPS VUI before sending.
    /// Mirrors `sendVideoFrame()` in `WebRtcWrapper.ts`.
    pub async fn send_video_frame(
        &mut self,
        frame: &[u8],
        frametime_ms: f64,
    ) -> Result<(), WebRtcError> {
        let track = self.video_track.as_ref().ok_or(WebRtcError::NotInitialized)?;
        let state = self.video_state.as_mut().ok_or(WebRtcError::PacketizerNotConfigured)?;
        let codec = self.video_codec.ok_or(WebRtcError::PacketizerNotConfigured)?;

        // H264: rewrite SPS VUI timing before packetization
        let processed: Vec<u8> = if codec == VideoCodec::H264 {
            rewrite_h264_sps_vui(frame)
        } else {
            frame.to_vec()
        };

        // DAVE E2EE encrypt
        let encrypted = {
            let mut dave = self.dave.lock().await;
            let dave_codec = match codec {
                VideoCodec::H264 => Codec::H264,
                VideoCodec::H265 => Codec::H265,
                VideoCodec::Vp8 => Codec::VP8,
                VideoCodec::Vp9 => Codec::VP9,
                VideoCodec::Av1 => Codec::AV1,
            };
            dave.encrypt(MediaType::VIDEO, dave_codec, &processed)
                .map_err(|e| WebRtcError::Dave(e.to_string()))?
        };

        let pkt = state.make_packet(&encrypted, true);
        state.advance_timestamp(frametime_ms);

        track
            .write(&Bytes::from(pkt))
            .await
            .map_err(|e| WebRtcError::Webrtc(e.to_string()))?;
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.peer_connection.as_ref().map_or(false, |pc| {
            pc.connection_state() == RTCPeerConnectionState::Connected
        })
    }

    pub async fn close(&mut self) {
        if let Some(pc) = self.peer_connection.take() {
            let _ = pc.close().await;
        }
        self.audio_track = None;
        self.video_track = None;
        self.audio_state = None;
        self.video_state = None;
    }

    pub fn peer_connection(&self) -> Option<&Arc<RTCPeerConnection>> {
        self.peer_connection.as_ref()
    }
}

/// Scan an Annex-B H264 frame for SPS NALUs and rewrite their VUI sections.
/// All other NALUs are passed through unchanged.
fn rewrite_h264_sps_vui(frame: &[u8]) -> Vec<u8> {
    let nalus = split_nalu(frame);
    let mut out = Vec::with_capacity(frame.len() + 32);

    for nalu in nalus {
        if nalu.is_empty() {
            continue;
        }
        let unit_type = H264Helpers::nal_unit_type(nalu);
        // Prepend 4-byte start code for each NALU
        out.extend_from_slice(&[0, 0, 0, 1]);
        if unit_type == H264NalUnitType::Sps as u8 {
            let rewritten = rewrite_sps_vui(nalu);
            out.extend_from_slice(&rewritten);
        } else {
            out.extend_from_slice(nalu);
        }
    }
    out
}
