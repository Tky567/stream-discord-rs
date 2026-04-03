use serde::Deserialize;

/// Inbound events dispatched from the Discord main gateway (op 0 DISPATCH).
/// Ported from `GatewayEvents.ts`.

// ---------------------------------------------------------------------------
// VOICE_STATE_UPDATE
// ---------------------------------------------------------------------------

/// Fired when the bot's own voice state changes.
/// We only care about `user_id` + `session_id` to start the voice connection.
#[derive(Debug, Clone, Deserialize)]
pub struct VoiceStateUpdate {
    pub user_id: String,
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// VOICE_SERVER_UPDATE
// ---------------------------------------------------------------------------

/// Fired after joining a voice channel — provides the voice server endpoint
/// and authentication token.
#[derive(Debug, Clone, Deserialize)]
pub struct VoiceServerUpdate {
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub endpoint: String,
    pub token: String,
}

// ---------------------------------------------------------------------------
// STREAM_CREATE  (Go Live)
// ---------------------------------------------------------------------------

/// Fired when a Go Live stream is created for the bot's user.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamCreate {
    pub stream_key: String,
    pub rtc_server_id: String,
}

// ---------------------------------------------------------------------------
// STREAM_SERVER_UPDATE  (Go Live)
// ---------------------------------------------------------------------------

/// Provides the stream server endpoint + token after `STREAM_CREATE`.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamServerUpdate {
    pub stream_key: String,
    pub endpoint: String,
    pub token: String,
}

// ---------------------------------------------------------------------------
// Dispatcher enum — wraps all events the Streamer cares about
// ---------------------------------------------------------------------------

/// Typed wrapper for raw-packet events received from the Discord gateway.
#[derive(Debug, Clone)]
pub enum GatewayEvent {
    VoiceStateUpdate(VoiceStateUpdate),
    VoiceServerUpdate(VoiceServerUpdate),
    StreamCreate(StreamCreate),
    StreamServerUpdate(StreamServerUpdate),
    /// Any other dispatch event we don't handle.
    Unknown(String),
}

impl GatewayEvent {
    /// Try to parse a raw gateway dispatch packet. `event_name` is the `t`
    /// field; `data` is the `d` field (already a `serde_json::Value`).
    pub fn from_dispatch(
        event_name: &str,
        data: serde_json::Value,
    ) -> Self {
        match event_name {
            "VOICE_STATE_UPDATE" => {
                if let Ok(v) = serde_json::from_value(data) {
                    return Self::VoiceStateUpdate(v);
                }
            }
            "VOICE_SERVER_UPDATE" => {
                if let Ok(v) = serde_json::from_value(data) {
                    return Self::VoiceServerUpdate(v);
                }
            }
            "STREAM_CREATE" => {
                if let Ok(v) = serde_json::from_value(data) {
                    return Self::StreamCreate(v);
                }
            }
            "STREAM_SERVER_UPDATE" => {
                if let Ok(v) = serde_json::from_value(data) {
                    return Self::StreamServerUpdate(v);
                }
            }
            _ => {}
        }
        Self::Unknown(event_name.to_owned())
    }
}
