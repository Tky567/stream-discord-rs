/// Ported from `utils.ts`.

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamType {
    Guild,
    Call,
}

impl std::fmt::Display for StreamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamType::Guild => write!(f, "guild"),
            StreamType::Call => write!(f, "call"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamKey {
    pub kind: StreamType,
    pub guild_id: Option<String>,
    pub channel_id: String,
    pub user_id: String,
}

#[derive(Debug, Error)]
pub enum StreamKeyError {
    #[error("invalid stream key type: {0}")]
    InvalidType(String),
    #[error("stream key too short: {0}")]
    TooShort(String),
}

/// Parse a Discord stream key.  
/// Format: `guild:<guild_id>:<channel_id>:<user_id>` or `call:<channel_id>:<user_id>`
pub fn parse_stream_key(key: &str) -> Result<StreamKey, StreamKeyError> {
    let mut parts: Vec<&str> = key.splitn(5, ':').collect();
    let kind_str = parts.remove(0);
    let kind = match kind_str {
        "guild" => StreamType::Guild,
        "call" => StreamType::Call,
        other => return Err(StreamKeyError::InvalidType(other.to_owned())),
    };

    match kind {
        StreamType::Guild => {
            if parts.len() < 3 {
                return Err(StreamKeyError::TooShort(key.to_owned()));
            }
            Ok(StreamKey {
                kind,
                guild_id: Some(parts[0].to_owned()),
                channel_id: parts[1].to_owned(),
                user_id: parts[2].to_owned(),
            })
        }
        StreamType::Call => {
            if parts.len() < 2 {
                return Err(StreamKeyError::TooShort(key.to_owned()));
            }
            Ok(StreamKey {
                kind,
                guild_id: None,
                channel_id: parts[0].to_owned(),
                user_id: parts[1].to_owned(),
            })
        }
    }
}

/// Generate a Discord stream key.
pub fn generate_stream_key(
    kind: StreamType,
    guild_id: Option<&str>,
    channel_id: &str,
    user_id: &str,
) -> String {
    match kind {
        StreamType::Guild => {
            format!(
                "guild:{}:{}:{}",
                guild_id.unwrap_or(""),
                channel_id,
                user_id
            )
        }
        StreamType::Call => {
            format!("call:{}:{}", channel_id, user_id)
        }
    }
}

// ---------------------------------------------------------------------------
// Video codec helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
}

impl std::fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoCodec::H264 => write!(f, "H264"),
            VideoCodec::H265 => write!(f, "H265"),
            VideoCodec::Vp8 => write!(f, "VP8"),
            VideoCodec::Vp9 => write!(f, "VP9"),
            VideoCodec::Av1 => write!(f, "AV1"),
        }
    }
}

#[derive(Debug, Error)]
#[error("unknown codec: {0}")]
pub struct UnknownCodecError(pub String);

/// Normalize a codec string to a [`VideoCodec`].
/// Mirrors `normalizeVideoCodec()` in `utils.ts`.
pub fn normalize_video_codec(s: &str) -> Result<VideoCodec, UnknownCodecError> {
    let upper = s.to_uppercase();
    if upper.contains("264") || upper.contains("AVC") {
        return Ok(VideoCodec::H264);
    }
    if upper.contains("265") || upper.contains("HEVC") {
        return Ok(VideoCodec::H265);
    }
    if upper.contains("VP8") {
        return Ok(VideoCodec::Vp8);
    }
    if upper.contains("VP9") {
        return Ok(VideoCodec::Vp9);
    }
    if upper.contains("AV1") {
        return Ok(VideoCodec::Av1);
    }
    Err(UnknownCodecError(s.to_owned()))
}

// ---------------------------------------------------------------------------
// Encryption mode constants  (SupportedEncryptionModes in utils.ts)
// ---------------------------------------------------------------------------

pub const ENCRYPTION_AES256: &str = "aead_aes256_gcm_rtpsize";
pub const ENCRYPTION_XCHACHA20: &str = "aead_xchacha20_poly1305_rtpsize";

// Hardcoded simulcast descriptor sent in IDENTIFY (STREAMS_SIMULCAST in utils.ts)
pub const STREAMS_SIMULCAST_RID: &str = "100";
pub const STREAMS_SIMULCAST_QUALITY: u8 = 100;

pub const MAX_INT16: u32 = 1 << 16;
pub const MAX_INT32: u64 = 1 << 32;
