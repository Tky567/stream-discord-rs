use crate::dave::{DaveError, DaveHandler};
use crate::voice::opcodes::{VoiceOpCode, VoiceOpCodeBinary};
use crate::voice::stream_connection::StreamConnection;
use crate::voice::types::{
    BinaryMessage, ClientDisconnect, ClientsConnect, DaveExecuteTransition, DavePrepareEpoch,
    DavePrepareTransition, GatewayMessage, Hello, Ready, SelectProtocolAck,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, warn};

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("DAVE error: {0}")]
    Dave(#[from] DaveError),
    #[error("Connection closed unexpectedly")]
    Closed,
    #[error("Not connected")]
    NotConnected,
}

/// Voice connection status — mirrors the TS `VoiceConnectionStatus` object.
#[derive(Debug, Default, Clone)]
pub struct ConnectionStatus {
    pub has_session: bool,
    pub has_token: bool,
    pub started: bool,
    pub resuming: bool,
}

/// The WebRTC + SSRC parameters received in the Ready message.
#[derive(Debug, Clone)]
pub struct WebRtcParams {
    pub address: String,
    pub port: u16,
    pub audio_ssrc: u32,
    pub video_ssrc: u32,
    pub rtx_ssrc: u32,
    pub supported_encryption_modes: Vec<String>,
}

/// Events emitted by the voice connection to callers.
#[derive(Debug)]
pub enum VoiceEvent {
    /// WebRTC params are ready — caller should set up ICE/DTLS.
    Ready(WebRtcParams),
    /// Discord sent a SELECT_PROTOCOL_ACK with an SDP answer.
    SelectProtocolAck { sdp: String, dave_version: u16 },
    /// Session resumed after disconnect.
    Resumed,
    /// A binary message that needs to be forwarded to the WebRTC layer
    /// (e.g. key package to send back as op 28).
    MlsCommitWelcome(Vec<u8>),
}

/// A single voice gateway connection.  
/// Corresponds to `BaseMediaConnection` in the TypeScript source.
pub struct VoiceConnection {
    guild_id: Option<String>,
    channel_id: String,
    bot_id: String,
    session_id: Option<String>,
    token: Option<String>,
    server: Option<String>,

    status: ConnectionStatus,
    seq: i64,

    dave: DaveHandler,
    connected_users: HashSet<String>,
    pending_transitions: HashMap<u64, u16>,

    /// Optional Go Live stream connection attached to this voice connection.
    pub stream_connection: Option<Box<StreamConnection>>,

    /// Channel for sending events out to whoever holds the connection.
    event_tx: mpsc::UnboundedSender<VoiceEvent>,
}

impl VoiceConnection {
    pub fn new(
        guild_id: Option<String>,
        channel_id: String,
        bot_id: String,
        event_tx: mpsc::UnboundedSender<VoiceEvent>,
    ) -> Self {
        let user_id: u64 = bot_id.parse().unwrap_or(0);
        let ch_id: u64 = channel_id.parse().unwrap_or(0);
        Self {
            guild_id,
            channel_id,
            bot_id,
            session_id: None,
            token: None,
            server: None,
            status: ConnectionStatus::default(),
            seq: -1,
            dave: DaveHandler::new(user_id, ch_id),
            connected_users: HashSet::new(),
            pending_transitions: HashMap::new(),
            stream_connection: None,
            event_tx,
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn stream_connection_mut(&mut self) -> Option<&mut StreamConnection> {
        self.stream_connection.as_deref_mut()
    }

    pub fn set_session(&mut self, session_id: String) {
        self.session_id = Some(session_id);
        self.status.has_session = true;
    }

    pub fn set_tokens(&mut self, server: String, token: String) {
        self.server = Some(server);
        self.token = Some(token);
        self.status.has_token = true;
    }

    /// Returns the server_id used for IDENTIFY (guild_id or channel_id for DMs).
    pub fn server_id(&self) -> &str {
        self.guild_id
            .as_deref()
            .unwrap_or_else(|| self.channel_id.as_str())
    }

    /// Return `true` if both gateway credentials have been received and the
    /// connection loop can start.
    pub fn can_start(&self) -> bool {
        self.status.has_session && self.status.has_token && !self.status.started
    }

    /// Connect to the voice gateway and start the event loop.
    /// Spawns a dedicated Tokio task; returns a handle.
    pub async fn start(
        mut self,
        resuming: bool,
    ) -> Result<tokio::task::JoinHandle<()>, ConnectionError> {
        let server = self
            .server
            .clone()
            .ok_or(ConnectionError::NotConnected)?;

        let url = format!("wss://{}/?v=8", server);
        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| ConnectionError::WebSocket(e.to_string()))?;

        debug!("Voice WS connected to {}", url);

        let (mut write, mut read) = ws_stream.split();
        self.status.started = true;
        self.status.resuming = resuming;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

        // Spawn writer task
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
        });

        // Build the IDENTIFY or RESUME payload immediately on connect
        let first_msg = if resuming {
            self.build_resume()
        } else {
            self.build_identify()
        };
        let _ = out_tx.send(Message::Text(first_msg.into()));

        let _event_tx = self.event_tx.clone();
        let out_tx_clone = out_tx.clone();

        let handle = tokio::spawn(async move {
            let mut heartbeat: Option<tokio::time::Interval> = None;

            loop {
                tokio::select! {
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                if let Err(e) = self.handle_text(&text, &out_tx_clone) {
                                    error!("Voice WS text error: {}", e);
                                }
                            }
                            Some(Ok(Message::Binary(data))) => {
                                if let Err(e) = self.handle_binary(&data, &out_tx_clone) {
                                    error!("Voice WS binary error: {}", e);
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                debug!("Voice WS closed");
                                break;
                            }
                            _ => {}
                        }
                    }
                    _ = async {
                        if let Some(ref mut iv) = heartbeat {
                            iv.tick().await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    } => {
                        let payload = json!({
                            "op": VoiceOpCode::Heartbeat as u8,
                            "d": { "t": Self::now_ms(), "seq_ack": self.seq }
                        });
                        let _ = out_tx_clone.send(Message::Text(payload.to_string().into()));
                    }
                }
            }
        });

        Ok(handle)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn build_identify(&self) -> String {
        json!({
            "op": VoiceOpCode::Identify as u8,
            "d": {
                "server_id": self.server_id(),
                "user_id": self.bot_id,
                "session_id": self.session_id,
                "token": self.token,
                "video": true,
                "streams": [{ "type": "screen", "rid": "100", "quality": 100 }]
            }
        })
        .to_string()
    }

    fn build_resume(&self) -> String {
        json!({
            "op": VoiceOpCode::Resume as u8,
            "d": {
                "server_id": self.server_id(),
                "session_id": self.session_id,
                "token": self.token,
                "seq_ack": self.seq
            }
        })
        .to_string()
    }

    fn send_opcode(
        &self,
        tx: &mpsc::UnboundedSender<Message>,
        op: VoiceOpCode,
        d: Value,
    ) {
        let msg = json!({ "op": op as u8, "d": d });
        let _ = tx.send(Message::Text(msg.to_string().into()));
    }

    fn send_binary(
        &self,
        tx: &mpsc::UnboundedSender<Message>,
        op: VoiceOpCodeBinary,
        seq: u16,
        payload: &[u8],
    ) {
        let frame = BinaryMessage::encode(seq, op.as_u8(), payload);
        let _ = tx.send(Message::Binary(frame.into()));
    }

    fn handle_text(
        &mut self,
        raw: &str,
        tx: &mpsc::UnboundedSender<Message>,
    ) -> Result<(), ConnectionError> {
        let msg: GatewayMessage = serde_json::from_str(raw)?;
        if let Some(seq) = msg.seq {
            self.seq = seq;
        }

        let op = VoiceOpCode::from_u8(msg.op);
        debug!("Voice WS op={:?}", op);

        match op {
            Some(VoiceOpCode::Hello) => {
                if let Ok(hello) = serde_json::from_value::<Hello>(msg.d) {
                    let interval_ms =
                        (hello.heartbeat_interval * 0.75) as u64;
                    debug!("Heartbeat interval {}ms", interval_ms);
                    // heartbeat timer is handled in the select! loop above
                }
            }
            Some(VoiceOpCode::Ready) => {
                let ready: Ready = serde_json::from_value(msg.d)?;
                let stream = &ready.streams[0];
                let params = WebRtcParams {
                    address: ready.ip.clone(),
                    port: ready.port,
                    audio_ssrc: ready.ssrc,
                    video_ssrc: stream.ssrc,
                    rtx_ssrc: stream.rtx_ssrc,
                    supported_encryption_modes: ready.modes.clone(),
                };
                let _ = self.event_tx.send(VoiceEvent::Ready(params));
            }
            Some(VoiceOpCode::SelectProtocolAck) => {
                let ack: SelectProtocolAck = serde_json::from_value(msg.d)?;
                if let Some(sdp) = ack.sdp {
                    let dave_version = ack.dave_protocol_version.unwrap_or(0);
                    // Initialize DAVE and send key package
                    if let Some(kp) = self.dave.init(dave_version)? {
                        self.send_binary(tx, VoiceOpCodeBinary::MlsKeyPackage, 0, &kp);
                    }
                    let _ = self
                        .event_tx
                        .send(VoiceEvent::SelectProtocolAck { sdp, dave_version });
                }
            }
            Some(VoiceOpCode::Resumed) => {
                self.status.started = true;
                let _ = self.event_tx.send(VoiceEvent::Resumed);
            }
            Some(VoiceOpCode::ClientsConnect) => {
                if let Ok(cc) = serde_json::from_value::<ClientsConnect>(msg.d) {
                    for uid in cc.user_ids {
                        self.connected_users.insert(uid);
                    }
                }
            }
            Some(VoiceOpCode::ClientDisconnect) => {
                if let Ok(cd) = serde_json::from_value::<ClientDisconnect>(msg.d) {
                    self.connected_users.remove(&cd.user_id);
                }
            }
            Some(VoiceOpCode::DavePrepareTransition) => {
                if let Ok(d) = serde_json::from_value::<DavePrepareTransition>(msg.d) {
                    debug!("DAVE prepare transition {:?}", d);
                    self.pending_transitions
                        .insert(d.transition_id, d.protocol_version);
                    if d.transition_id == 0 {
                        self.execute_pending_transition(0);
                    } else {
                        if d.protocol_version == 0 {
                            self.dave.set_passthrough_mode(true, Some(120));
                        }
                        self.send_opcode(
                            tx,
                            VoiceOpCode::DaveTransitionReady,
                            json!({ "transition_id": d.transition_id }),
                        );
                    }
                }
            }
            Some(VoiceOpCode::DaveExecuteTransition) => {
                if let Ok(d) = serde_json::from_value::<DaveExecuteTransition>(msg.d) {
                    self.execute_pending_transition(d.transition_id);
                }
            }
            Some(VoiceOpCode::DavePrepareEpoch) => {
                if let Ok(d) = serde_json::from_value::<DavePrepareEpoch>(msg.d) {
                    debug!("DAVE prepare epoch {:?}", d);
                    if d.epoch == 1 {
                        if let Some(kp) = self.dave.init(d.protocol_version)? {
                            self.send_binary(tx, VoiceOpCodeBinary::MlsKeyPackage, 0, &kp);
                        }
                    }
                }
            }
            Some(VoiceOpCode::HeartbeatAck) | Some(VoiceOpCode::Speaking) => {}
            Some(_op) if (msg.op as u16) >= 4000 => {
                error!("Voice gateway error op={}", msg.op);
            }
            _ => {
                debug!("Unhandled voice op {}", msg.op);
            }
        }

        Ok(())
    }

    fn handle_binary(
        &mut self,
        data: &[u8],
        tx: &mpsc::UnboundedSender<Message>,
    ) -> Result<(), ConnectionError> {
        let frame = match BinaryMessage::parse(data) {
            Some(f) => f,
            None => return Ok(()),
        };
        self.seq = frame.seq as i64;

        match VoiceOpCodeBinary::from_u8(frame.op) {
            Some(VoiceOpCodeBinary::MlsExternalSender) => {
                self.dave.set_external_sender(&frame.payload)?;
            }
            Some(VoiceOpCodeBinary::MlsProposals) => {
                if frame.payload.is_empty() {
                    return Ok(());
                }
                let op_type = frame.payload[0];
                let proposals = &frame.payload[1..];
                let user_ids: Vec<u64> = self
                    .connected_users
                    .iter()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if let Some(cw) = self.dave.process_proposals(op_type, proposals, &user_ids)? {
                    // Build op 28 payload: commit || welcome
                    let mut payload = cw.commit.clone();
                    if let Some(w) = cw.welcome {
                        payload.extend_from_slice(&w);
                    }
                    self.send_binary(tx, VoiceOpCodeBinary::MlsCommitWelcome, frame.seq, &payload);
                    let _ = self.event_tx.send(VoiceEvent::MlsCommitWelcome(payload));
                }
            }
            Some(VoiceOpCodeBinary::MlsAnnounceCommitTransition) => {
                if frame.payload.len() < 2 {
                    return Ok(());
                }
                let transition_id = u16::from_be_bytes([frame.payload[0], frame.payload[1]]) as u64;
                let commit_data = &frame.payload[2..];
                match self.dave.process_commit(commit_data) {
                    Ok(()) => {
                        if transition_id > 0 {
                            self.pending_transitions
                                .insert(transition_id, self.dave.protocol_version());
                            self.send_opcode(
                                tx,
                                VoiceOpCode::DaveTransitionReady,
                                json!({ "transition_id": transition_id }),
                            );
                        }
                        debug!("DAVE MLS commit processed (transition {})", transition_id);
                    }
                    Err(e) => {
                        warn!("DAVE MLS commit error: {}", e);
                        self.process_invalid_commit(transition_id, tx);
                    }
                }
            }
            Some(VoiceOpCodeBinary::MlsWelcome) => {
                if frame.payload.len() < 2 {
                    return Ok(());
                }
                let transition_id = u16::from_be_bytes([frame.payload[0], frame.payload[1]]) as u64;
                let welcome_data = &frame.payload[2..];
                match self.dave.process_welcome(welcome_data) {
                    Ok(()) => {
                        if transition_id > 0 {
                            self.pending_transitions
                                .insert(transition_id, self.dave.protocol_version());
                            self.send_opcode(
                                tx,
                                VoiceOpCode::DaveTransitionReady,
                                json!({ "transition_id": transition_id }),
                            );
                        }
                        debug!("DAVE MLS welcome processed (transition {})", transition_id);
                    }
                    Err(e) => {
                        warn!("DAVE MLS welcome error: {}", e);
                        self.process_invalid_commit(transition_id, tx);
                    }
                }
            }
            _ => {
                debug!("Unhandled binary voice op {}", frame.op);
            }
        }

        Ok(())
    }

    fn execute_pending_transition(&mut self, transition_id: u64) {
        if let Some(new_version) = self.pending_transitions.remove(&transition_id) {
            let old_version = self.dave.protocol_version();
            // Downgrade → passthrough
            if old_version != 0 && new_version == 0 {
                self.dave.set_passthrough_mode(true, Some(10));
                debug!("DAVE: Downgraded to non-E2EE (transition {})", transition_id);
            } else if transition_id > 0 && old_version == 0 && new_version != 0 {
                self.dave.set_passthrough_mode(true, Some(10));
                debug!("DAVE: Upgraded to E2EE (transition {})", transition_id);
            }
            debug!("DAVE: Executed pending transition {}", transition_id);
        } else {
            warn!("DAVE: Unrecognized transition ID {}", transition_id);
        }
    }

    fn process_invalid_commit(
        &mut self,
        transition_id: u64,
        tx: &mpsc::UnboundedSender<Message>,
    ) {
        warn!(
            "DAVE: Invalid commit (transition {}), reinitializing",
            transition_id
        );
        self.send_opcode(
            tx,
            VoiceOpCode::MlsInvalidCommitWelcome,
            json!({ "transition_id": transition_id }),
        );
        // Reinit will send a new key package automatically
        if let Ok(Some(kp)) = self.dave.init(self.dave.protocol_version()) {
            self.send_binary(tx, VoiceOpCodeBinary::MlsKeyPackage, 0, &kp);
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
