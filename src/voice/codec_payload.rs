/// Codec payload type constants — ported from `CodecPayloadType.ts`.
/// These are the fixed RTP payload type numbers Discord uses.

#[derive(Debug, Clone, Copy)]
pub struct AudioCodecInfo {
    pub name: &'static str,
    pub clock_rate: u32,
    pub payload_type: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct VideoCodecInfo {
    pub name: &'static str,
    pub clock_rate: u32,
    pub payload_type: u8,
    pub rtx_payload_type: u8,
    pub priority: u16,
}

pub const OPUS: AudioCodecInfo = AudioCodecInfo {
    name: "opus",
    clock_rate: 48_000,
    payload_type: 120,
};

pub const H264: VideoCodecInfo = VideoCodecInfo {
    name: "H264",
    clock_rate: 90_000,
    priority: 1000,
    payload_type: 101,
    rtx_payload_type: 102,
};

pub const H265: VideoCodecInfo = VideoCodecInfo {
    name: "H265",
    clock_rate: 90_000,
    priority: 1000,
    payload_type: 103,
    rtx_payload_type: 104,
};

pub const VP8: VideoCodecInfo = VideoCodecInfo {
    name: "VP8",
    clock_rate: 90_000,
    priority: 1000,
    payload_type: 105,
    rtx_payload_type: 106,
};

pub const VP9: VideoCodecInfo = VideoCodecInfo {
    name: "VP9",
    clock_rate: 90_000,
    priority: 1000,
    payload_type: 107,
    rtx_payload_type: 108,
};

pub const AV1: VideoCodecInfo = VideoCodecInfo {
    name: "AV1",
    clock_rate: 90_000,
    priority: 1000,
    payload_type: 109,
    rtx_payload_type: 110,
};

pub const ALL_VIDEO_CODECS: &[VideoCodecInfo] = &[H264, H265, VP8, VP9, AV1];
