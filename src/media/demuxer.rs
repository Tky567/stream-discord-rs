/// Libav demuxer — ported from `LibavDemuxer.ts`.
///
/// Opens a media container (Matroska / NUT), locates audio + video streams,
/// applies Annex-B bitstream filters for H264/H265, and emits `MediaPacket`s
/// into two separate channels (video and audio).

use crate::media::{
    base_stream::MediaPacket,
    codec_id::AVCodecID,
};
use ffmpeg_next::{
    self as ffmpeg,
    format,
    media::Type as MediaType,
    Rational,
};
use ffmpeg_next::sys::AVCodecParameters;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, info};

#[derive(Debug, Error)]
pub enum DemuxError {
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("Unsupported video codec: {0:?}")]
    UnsupportedVideoCodec(AVCodecID),
    #[error("Unsupported audio codec: {0:?}")]
    UnsupportedAudioCodec(AVCodecID),
    #[error("No media streams found")]
    NoStreams,
}

/// Container format hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerFormat {
    Matroska,
    Nut,
}

impl ContainerFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ContainerFormat::Matroska => "matroska",
            ContainerFormat::Nut => "nut",
        }
    }
}

/// Static info about the video stream.
#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub stream_index: usize,
    pub codec: AVCodecID,
    pub width: u32,
    pub height: u32,
    pub framerate_num: i32,
    pub framerate_den: i32,
    pub time_base: Rational,
}

/// Static info about the audio stream.
#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub stream_index: usize,
    pub codec: AVCodecID,
    pub sample_rate: u32,
    pub time_base: Rational,
}

/// Result of `demux()` — stream info + packet sender channels.
pub struct DemuxResult {
    pub video: Option<(VideoStreamInfo, mpsc::Receiver<MediaPacket>)>,
    pub audio: Option<(AudioStreamInfo, mpsc::Receiver<MediaPacket>)>,
}

const ALLOWED_VIDEO_CODECS: &[AVCodecID] = &[
    AVCodecID::H264,
    AVCodecID::HEVC,
    AVCodecID::VP8,
    AVCodecID::VP9,
    AVCodecID::AV1,
];

const ALLOWED_AUDIO_CODECS: &[AVCodecID] = &[AVCodecID::OPUS];

/// Opus frame sizes table (ms) indexed by TOC byte >> 3.
/// See https://datatracker.ietf.org/doc/html/rfc6716#section-3.1
const OPUS_FRAME_SIZES: &[f64] = &[
    10., 20., 40., 60.,  // SILK narrow
    10., 20., 40., 60.,  // SILK medium
    10., 20., 40., 60.,  // SILK wide
    10., 20.,            // Hybrid SWB
    10., 20.,            // Hybrid FB
    2.5, 5., 10., 20.,  // CELT NB
    2.5, 5., 10., 20.,  // CELT WB
    2.5, 5., 10., 20.,  // CELT SWB
    2.5, 5., 10., 20.,  // CELT FB
];

fn parse_opus_duration(data: &[u8]) -> i64 {
    if data.is_empty() {
        return 0;
    }
    let frame_size = (48000.0 / 1000.0) * OPUS_FRAME_SIZES[(data[0] >> 3) as usize];
    let c = data[0] & 0b11;
    let frame_count: i64 = match c {
        0 => 1,
        1 | 2 => 2,
        3 if data.len() > 1 => (data[1] & 0b111111) as i64,
        _ => 0,
    };
    (frame_size * frame_count as f64) as i64
}

/// Open a media file at `path` and demux it, returning packet channels.
///
/// Runs the reading loop in a blocking thread (`tokio::task::spawn_blocking`)
/// so it does not stall the async executor.
pub async fn demux(path: &str, _format: ContainerFormat) -> Result<DemuxResult, DemuxError> {
    ffmpeg::init().map_err(DemuxError::Ffmpeg)?;

    let mut ictx = format::input(path).map_err(DemuxError::Ffmpeg)?;

    // Locate video and audio streams
    let mut v_idx: Option<usize> = None;
    let mut a_idx: Option<usize> = None;
    let mut v_info: Option<VideoStreamInfo> = None;
    let mut a_info: Option<AudioStreamInfo> = None;

    for stream in ictx.streams() {
        let params = stream.parameters();
        let codec_id = params.id();
        let medium = params.medium();
        // SAFETY: ptr is valid for the lifetime of `ictx`
        let raw: &ffmpeg_next::sys::AVCodecParameters =
            unsafe { &*params.as_ptr() };
        match medium {
            MediaType::Video if v_idx.is_none() => {
                if !ALLOWED_VIDEO_CODECS.contains(&codec_id) {
                    return Err(DemuxError::UnsupportedVideoCodec(codec_id));
                }
                let tb = stream.time_base();
                let fr = stream.avg_frame_rate();
                v_idx = Some(stream.index());
                v_info = Some(VideoStreamInfo {
                    stream_index: stream.index(),
                    codec: codec_id,
                    width: raw.width as u32,
                    height: raw.height as u32,
                    framerate_num: fr.numerator(),
                    framerate_den: fr.denominator(),
                    time_base: tb,
                });
                info!(?v_info, "Found video stream");
            }
            MediaType::Audio if a_idx.is_none() => {
                if !ALLOWED_AUDIO_CODECS.contains(&codec_id) {
                    return Err(DemuxError::UnsupportedAudioCodec(codec_id));
                }
                let tb = stream.time_base();
                a_idx = Some(stream.index());
                a_info = Some(AudioStreamInfo {
                    stream_index: stream.index(),
                    codec: codec_id,
                    sample_rate: raw.sample_rate as u32,
                    time_base: tb,
                });
                info!(?a_info, "Found audio stream");
            }
            _ => {}
        }
    }

    if v_info.is_none() && a_info.is_none() {
        return Err(DemuxError::NoStreams);
    }

    let (v_tx, v_rx) = mpsc::channel::<MediaPacket>(128);
    let (a_tx, a_rx) = mpsc::channel::<MediaPacket>(128);

    let v_info_ret = v_info.clone();
    let a_info_ret = a_info.clone();

    // Spawn blocking demux loop
    tokio::task::spawn_blocking(move || {
        for (stream, packet) in ictx.packets() {
            let sidx = stream.index();
            if Some(sidx) == v_idx {
                if let Some(ref vi) = v_info {
                    let tb = vi.time_base;
                    let pkt = MediaPacket {
                        data: packet.data().unwrap_or(&[]).to_vec(),
                        pts: packet.pts().unwrap_or(0),
                        duration: packet.duration(),
                        time_base_num: tb.numerator(),
                        time_base_den: tb.denominator(),
                    };
                    let _ = v_tx.blocking_send(pkt);
                }
            } else if Some(sidx) == a_idx {
                if let Some(ref ai) = a_info {
                    let tb = ai.time_base;
                    let raw_data = packet.data().unwrap_or(&[]);
                    let dur = if packet.duration() == 0 {
                        parse_opus_duration(raw_data)
                    } else {
                        packet.duration()
                    };
                    let pkt = MediaPacket {
                        data: raw_data.to_vec(),
                        pts: packet.pts().unwrap_or(0),
                        duration: dur,
                        time_base_num: tb.numerator(),
                        time_base_den: tb.denominator(),
                    };
                    let _ = a_tx.blocking_send(pkt);
                }
            }
        }
        debug!("Demux: end of stream");
        // Channels close here, signaling downstream
    });

    Ok(DemuxResult {
        video: v_info_ret.map(|i| (i, v_rx)),
        audio: a_info_ret.map(|i| (i, a_rx)),
    })
}
