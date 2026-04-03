use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Outbound (client → server)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Identify {
    pub server_id: String,
    pub user_id: String,
    pub session_id: String,
    pub token: String,
    pub video: bool,
    pub streams: Vec<StreamDesc>,
}

#[derive(Debug, Serialize)]
pub struct StreamDesc {
    #[serde(rename = "type")]
    pub kind: String, // "screen"
    pub rid: String,  // "100"
    pub quality: u8,  // 100
}

#[derive(Debug, Serialize)]
pub struct Resume {
    pub server_id: String,
    pub session_id: String,
    pub token: String,
    pub seq_ack: i64,
}

#[derive(Debug, Serialize)]
pub struct Heartbeat {
    pub t: u64,
    pub seq_ack: i64,
}

#[derive(Debug, Serialize)]
pub struct SelectProtocol {
    pub protocol: String, // "webrtc"
    pub data: SelectProtocolData,
    pub sdp: String,
    pub codecs: Vec<CodecInfo>,
    pub experiments: Vec<String>,
    pub dave_protocol_version: u16,
    pub address: String,
    pub port: u16,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct SelectProtocolData {
    pub address: String,
    pub port: u16,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct CodecInfo {
    pub name: String,
    pub payload_type: u8,
    #[serde(rename = "type")]
    pub kind: String,
    pub priority: u16,
    pub rtx_payload_type: Option<u8>,
}

#[derive(Debug, Serialize)]
pub struct Speaking {
    pub speaking: u8,
    pub delay: u32,
    pub ssrc: u32,
}

#[derive(Debug, Serialize)]
pub struct VideoAttributes {
    pub audio_ssrc: u32,
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
    pub streams: Vec<VideoStream>,
}

#[derive(Debug, Serialize)]
pub struct VideoStream {
    pub active: bool,
    pub description: Option<String>,
    pub quality: u8,
    pub rid: String,
    pub rtx_ssrc: u32,
    pub ssrc: u32,
}

#[derive(Debug, Serialize)]
pub struct DaveTransitionReady {
    pub transition_id: u64,
}

#[derive(Debug, Serialize)]
pub struct MlsInvalidCommitWelcome {
    pub transition_id: u64,
}

// ---------------------------------------------------------------------------
// Inbound (server → client)
// ---------------------------------------------------------------------------

/// Outer envelope for all JSON voice gateway messages.
#[derive(Debug, Deserialize)]
pub struct GatewayMessage {
    pub op: u8,
    pub d: serde_json::Value,
    pub seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct Hello {
    pub heartbeat_interval: f64,
}

#[derive(Debug, Deserialize)]
pub struct Ready {
    pub ssrc: u32,
    pub ip: String,
    pub port: u16,
    pub modes: Vec<String>,
    pub streams: Vec<ReadyStream>,
}

#[derive(Debug, Deserialize)]
pub struct ReadyStream {
    pub ssrc: u32,
    pub rtx_ssrc: u32,
}

#[derive(Debug, Deserialize)]
pub struct SelectProtocolAck {
    pub sdp: Option<String>,
    pub dave_protocol_version: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct ClientsConnect {
    pub user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClientDisconnect {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DavePrepareTransition {
    pub transition_id: u64,
    pub protocol_version: u16,
}

#[derive(Debug, Deserialize)]
pub struct DaveExecuteTransition {
    pub transition_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct DavePrepareEpoch {
    pub epoch: u64,
    pub protocol_version: u16,
}

// ---------------------------------------------------------------------------
// Binary message frame (seq:u16 + op:u8 + payload)
// ---------------------------------------------------------------------------

pub struct BinaryMessage {
    pub seq: u16,
    pub op: u8,
    pub payload: Vec<u8>,
}

impl BinaryMessage {
    /// Parse a raw binary frame from the voice gateway.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        let seq = u16::from_be_bytes([data[0], data[1]]);
        let op = data[2];
        let payload = data[3..].to_vec();
        Some(Self { seq, op, payload })
    }

    /// Encode a binary frame to send to the voice gateway.
    pub fn encode(seq: u16, op: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + payload.len());
        out.extend_from_slice(&seq.to_be_bytes());
        out.push(op);
        out.extend_from_slice(payload);
        out
    }
}
