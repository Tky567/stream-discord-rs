/// JSON-based Voice Gateway opcodes.
/// Source: discord.js-selfbot-v13 / Discord developer docs.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceOpCode {
    Identify = 0,
    SelectProtocol = 1,
    Ready = 2,
    Heartbeat = 3,
    SelectProtocolAck = 4,
    Speaking = 5,
    HeartbeatAck = 6,
    Resume = 7,
    Hello = 8,
    Resumed = 9,
    ClientsConnect = 11,
    Video = 12,
    ClientDisconnect = 13,
    SessionUpdate = 14,
    MediaSinkWants = 15,
    VoiceBackendVersion = 16,
    ChannelOptionsUpdate = 17,
    Flags = 18,
    SpeedTest = 19,
    Platform = 20,
    DavePrepareTransition = 21,
    DaveExecuteTransition = 22,
    DaveTransitionReady = 23,
    DavePrepareEpoch = 24,
    MlsInvalidCommitWelcome = 31,
}

impl VoiceOpCode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Identify),
            1 => Some(Self::SelectProtocol),
            2 => Some(Self::Ready),
            3 => Some(Self::Heartbeat),
            4 => Some(Self::SelectProtocolAck),
            5 => Some(Self::Speaking),
            6 => Some(Self::HeartbeatAck),
            7 => Some(Self::Resume),
            8 => Some(Self::Hello),
            9 => Some(Self::Resumed),
            11 => Some(Self::ClientsConnect),
            12 => Some(Self::Video),
            13 => Some(Self::ClientDisconnect),
            14 => Some(Self::SessionUpdate),
            15 => Some(Self::MediaSinkWants),
            16 => Some(Self::VoiceBackendVersion),
            17 => Some(Self::ChannelOptionsUpdate),
            18 => Some(Self::Flags),
            19 => Some(Self::SpeedTest),
            20 => Some(Self::Platform),
            21 => Some(Self::DavePrepareTransition),
            22 => Some(Self::DaveExecuteTransition),
            23 => Some(Self::DaveTransitionReady),
            24 => Some(Self::DavePrepareEpoch),
            31 => Some(Self::MlsInvalidCommitWelcome),
            _ => None,
        }
    }
}

/// Binary-framed Voice Gateway opcodes (header: 2-byte seq + 1-byte op).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceOpCodeBinary {
    MlsExternalSender = 25,
    MlsKeyPackage = 26,
    MlsProposals = 27,
    MlsCommitWelcome = 28,
    MlsAnnounceCommitTransition = 29,
    MlsWelcome = 30,
}

impl VoiceOpCodeBinary {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            25 => Some(Self::MlsExternalSender),
            26 => Some(Self::MlsKeyPackage),
            27 => Some(Self::MlsProposals),
            28 => Some(Self::MlsCommitWelcome),
            29 => Some(Self::MlsAnnounceCommitTransition),
            30 => Some(Self::MlsWelcome),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}
