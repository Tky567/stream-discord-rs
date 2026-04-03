use crate::voice::connection::VoiceConnection;
use crate::voice::opcodes::VoiceOpCode;
use serde_json::json;
use tokio::sync::mpsc;
use crate::voice::connection::VoiceEvent;

/// A second voice WebSocket connection used for Go Live streams.
/// Mirrors `StreamConnection.ts` which extends `BaseMediaConnection`.
pub struct StreamConnection {
    /// Underlying voice connection state machine (reuses all DAVE + WS logic).
    pub inner: VoiceConnection,
    /// The Discord stream key (e.g. `guild:<guild>:<ch>:<user>`).
    pub stream_key: Option<String>,
    /// The RTC server ID used to derive `daveChannelId`.
    pub server_id: Option<String>,
}

impl StreamConnection {
    pub fn new(
        guild_id: Option<String>,
        channel_id: String,
        bot_id: String,
        event_tx: mpsc::UnboundedSender<VoiceEvent>,
    ) -> Self {
        Self {
            inner: VoiceConnection::new(guild_id, channel_id, bot_id, event_tx),
            stream_key: None,
            server_id: None,
        }
    }

    /// The DAVE channel id for a stream is `rtc_server_id - 1`.
    /// Mirrors `StreamConnection.daveChannelId` getter in TS.
    pub fn dave_channel_id(&self) -> Option<u64> {
        self.server_id
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|id| id.saturating_sub(1))
    }

    /// The `serverId` used in IDENTIFY for a stream connection equals the
    /// raw RTC server ID (not the guild id).
    pub fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }
}
