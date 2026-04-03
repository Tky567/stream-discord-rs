/// Thin wrapper around the `davey` crate's [`DaveSession`] that mirrors
/// the JavaScript `@snazzah/davey` `DAVESession` API used in
/// `BaseMediaConnection.ts`.
///
/// It handles:
/// - init / reinit / reset of the MLS session
/// - processing binary opcodes 25-30 from the voice gateway
/// - encrypting outbound Opus/video frames with E2EE
/// - decrypting inbound frames from other users
use davey::{Codec, DaveSession, MediaType, ProposalsOperationType, SessionStatus};
use std::num::NonZeroU16;
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum DaveError {
    #[error("davey init error: {0}")]
    Init(String),
    #[error("davey reinit error: {0}")]
    Reinit(String),
    #[error("davey reset error: {0}")]
    Reset(String),
    #[error("davey key package error: {0}")]
    KeyPackage(String),
    #[error("davey set external sender error: {0}")]
    SetExternalSender(String),
    #[error("davey process proposals error: {0}")]
    ProcessProposals(String),
    #[error("davey process commit error: {0}")]
    ProcessCommit(String),
    #[error("davey process welcome error: {0}")]
    ProcessWelcome(String),
    #[error("davey encrypt error: {0}")]
    Encrypt(String),
    #[error("davey decrypt error: {0}")]
    Decrypt(String),
    #[error("invalid protocol version: 0")]
    InvalidProtocolVersion,
}

/// Result type returned from a [`processProposals`] call when a commit (and
/// sometimes a welcome) needs to be sent back to Discord.
pub struct CommitWelcome {
    /// The serialized MLS commit message (op 28 payload prefix).
    pub commit: Vec<u8>,
    /// The serialized MLS welcome message, appended after `commit` in op 28.
    pub welcome: Option<Vec<u8>>,
}

pub struct DaveHandler {
    inner: Option<DaveSession>,
    protocol_version: u16,
    user_id: u64,
    channel_id: u64,
}

impl DaveHandler {
    pub fn new(user_id: u64, channel_id: u64) -> Self {
        Self {
            inner: None,
            protocol_version: 0,
            user_id,
            channel_id,
        }
    }

    /// Initialize (or reinitialize) the DAVE session for the given protocol
    /// version. If `protocol_version == 0` the session is reset and put into
    /// passthrough mode (non-E2EE call).
    pub fn init(&mut self, protocol_version: u16) -> Result<Option<Vec<u8>>, DaveError> {
        self.protocol_version = protocol_version;

        if protocol_version == 0 {
            if let Some(s) = &mut self.inner {
                s.reset().map_err(|e| DaveError::Reset(e.to_string()))?;
                s.set_passthrough_mode(true, Some(10));
            }
            debug!("DAVE: Non-E2EE call (v0), passthrough enabled");
            return Ok(None);
        }

        let version =
            NonZeroU16::new(protocol_version).ok_or(DaveError::InvalidProtocolVersion)?;

        if let Some(s) = &mut self.inner {
            s.reinit(version, self.user_id, self.channel_id, None)
                .map_err(|e| DaveError::Reinit(e.to_string()))?;
            debug!("DAVE: Reinitialized session v{}", protocol_version);
        } else {
            let session = DaveSession::new(version, self.user_id, self.channel_id, None)
                .map_err(|e| DaveError::Init(e.to_string()))?;
            self.inner = Some(session);
            debug!("DAVE: Initialized new session v{}", protocol_version);
        }

        // Return serialized key package for op MLS_KEY_PACKAGE (26)
        let kp = self
            .inner
            .as_mut()
            .unwrap()
            .create_key_package()
            .map_err(|e| DaveError::KeyPackage(e.to_string()))?;
        Ok(Some(kp))
    }

    /// Handle binary op 25: MLS_EXTERNAL_SENDER
    pub fn set_external_sender(&mut self, data: &[u8]) -> Result<(), DaveError> {
        if let Some(s) = &mut self.inner {
            s.set_external_sender(data)
                .map_err(|e| DaveError::SetExternalSender(e.to_string()))?;
            debug!("DAVE: External sender set");
        }
        Ok(())
    }

    /// Handle binary op 27: MLS_PROPOSALS
    /// Returns `Some(CommitWelcome)` if we need to send op 28 back.
    pub fn process_proposals(
        &mut self,
        op_type: u8,
        proposals: &[u8],
        connected_users: &[u64],
    ) -> Result<Option<CommitWelcome>, DaveError> {
        let session = match &mut self.inner {
            Some(s) => s,
            None => return Ok(None),
        };

        let operation = match op_type {
            0 => ProposalsOperationType::APPEND,
            1 => ProposalsOperationType::REVOKE,
            _ => ProposalsOperationType::APPEND,
        };

        let expected: Option<&[u64]> = if connected_users.is_empty() {
            None
        } else {
            Some(connected_users)
        };

        let result = session
            .process_proposals(operation, proposals, expected)
            .map_err(|e| DaveError::ProcessProposals(e.to_string()))?;

        debug!("DAVE: Processed MLS proposals");

        Ok(result.map(|cw| CommitWelcome {
            commit: cw.commit.to_vec(),
            welcome: cw.welcome.map(|w| w.to_vec()),
        }))
    }

    /// Handle binary op 29: MLS_ANNOUNCE_COMMIT_TRANSITION
    pub fn process_commit(&mut self, data: &[u8]) -> Result<(), DaveError> {
        if let Some(s) = &mut self.inner {
            s.process_commit(data)
                .map_err(|e| DaveError::ProcessCommit(e.to_string()))?;
            debug!("DAVE: MLS commit processed");
        }
        Ok(())
    }

    /// Handle binary op 30: MLS_WELCOME
    pub fn process_welcome(&mut self, data: &[u8]) -> Result<(), DaveError> {
        if let Some(s) = &mut self.inner {
            s.process_welcome(data)
                .map_err(|e| DaveError::ProcessWelcome(e.to_string()))?;
            debug!("DAVE: MLS welcome processed");
        }
        Ok(())
    }

    /// Enable/disable passthrough mode (used during E2EE transitions).
    pub fn set_passthrough_mode(&mut self, enabled: bool, expiry_secs: Option<u32>) {
        if let Some(s) = &mut self.inner {
            s.set_passthrough_mode(enabled, expiry_secs);
        }
    }

    /// Encrypt an Opus audio frame for E2EE (before SRTP transport encryption).
    /// Returns the original packet unchanged if the session is not ready.
    pub fn encrypt_opus<'a>(&mut self, packet: &'a [u8]) -> Result<Vec<u8>, DaveError> {
        match &mut self.inner {
            Some(s) if s.is_ready() => {
                let out = s
                    .encrypt_opus(packet)
                    .map_err(|e| DaveError::Encrypt(e.to_string()))?;
                Ok(out.into_owned())
            }
            _ => Ok(packet.to_vec()),
        }
    }

    /// Encrypt any media frame (video/audio with explicit codec).
    /// Returns the original packet unchanged if the session is not ready.
    pub fn encrypt(&mut self, media_type: MediaType, codec: Codec, packet: &[u8]) -> Result<Vec<u8>, DaveError> {
        match &mut self.inner {
            Some(s) if s.is_ready() => {
                let out = s
                    .encrypt(media_type, codec, packet)
                    .map_err(|e| DaveError::Encrypt(e.to_string()))?;
                Ok(out.into_owned())
            }
            _ => Ok(packet.to_vec()),
        }
    }

    /// Decrypt an incoming media frame from `user_id`.
    /// Returns the original packet unchanged if the session is not ready.
    pub fn decrypt(
        &mut self,
        user_id: u64,
        media_type: MediaType,
        packet: &[u8],
    ) -> Result<Vec<u8>, DaveError> {
        match &mut self.inner {
            Some(s) if s.is_ready() => s
                .decrypt(user_id, media_type, packet)
                .map_err(|e| DaveError::Decrypt(e.to_string())),
            _ => Ok(packet.to_vec()),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.inner.as_ref().map_or(false, |s| s.is_ready())
    }

    pub fn status(&self) -> Option<SessionStatus> {
        self.inner.as_ref().map(|s| s.status())
    }

    pub fn voice_privacy_code(&self) -> Option<&str> {
        self.inner.as_ref().and_then(|s| s.voice_privacy_code())
    }

    pub fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Whether the connection is running in non-E2EE (passthrough) mode.
    pub fn is_passthrough(&self) -> bool {
        self.protocol_version == 0
    }
}
