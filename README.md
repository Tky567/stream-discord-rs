# discord-stream-rs

A Rust library for streaming audio/video to Discord voice channels via selfbot.  
Port of [`@dank074/discord-video-stream`](https://github.com/dank074/discord-video-stream) (TypeScript) — same architecture, same protocol, native Rust performance.

> **⚠️ Warning:** Using selfbots violates [Discord's Terms of Service](https://discord.com/terms). Use at your own risk.

---

## Features

- **DAVE / E2EE** — Full Discord end-to-end encryption via the [`davey`](https://crates.io/crates/davey) crate (itself written in Rust)
- **WebRTC** — ICE negotiation, DTLS-SRTP, RTP packetization via [`webrtc-rs`](https://crates.io/crates/webrtc)
- **All video codecs** — H264, H265, VP8, VP9, AV1
- **Hardware encoders** — NVENC (NVIDIA), VA-API (Intel/AMD), or software (x264/x265)
- **Media pipeline** — Demux + decode any container via FFmpeg (`ffmpeg-next`)
- **Go Live / Screen Share** — `StreamConnection` for guild streams
- **AV sync** — PTS-based timing with cross-stream synchronization
- **H264 SPS VUI rewriting** — Automatically patches `max_num_reorder_frames = 0` for real-time playback

---

## Requirements

**System packages** (Ubuntu/Debian):
```bash
sudo apt install libavcodec-dev libavformat-dev libavfilter-dev \
                 libavdevice-dev libswscale-dev libswresample-dev \
                 clang libclang-dev
```

**Rust:** edition 2024 (Rust ≥ 1.85)

---

## Installation

```toml
[dependencies]
discord-stream-rs = "0.1"
tokio = { version = "1", features = ["full"] }
```

Add to `.cargo/config.toml` in your project (required for FFmpeg bindings):
```toml
[env]
LIBCLANG_PATH = "/usr/lib/llvm-18/lib"
```

---

## Architecture Overview

The library is **transport-agnostic** — it does not manage the main Discord gateway WebSocket. Instead:

1. **Your code** maintains the main gateway WS connection
2. You feed incoming dispatch events into [`Streamer::handle_event`]
3. The `Streamer` pushes outbound [`GatewayPayload`]s back to you via an `mpsc` channel
4. You write those payloads to the main gateway WS

This allows the library to work with any Discord WS client (custom, serenity, etc.).

```
Your WS Client  ──dispatch events──▶  Streamer::handle_event()
                ◀── GatewayPayload ──  gateway_tx (mpsc channel)
                                           │
                                     VoiceConnection
                                           │ (voice WS, ICE, DTLS)
                                     WebRtcWrapper
                                           │ (RTP + DAVE E2EE)
                                    ┌──────┴──────┐
                               AudioStream    VideoStream
                                    │               │
                               send_audio_frame  send_video_frame
```

---

## Quick Start

### 1. Basic setup

```rust
use discord_stream_rs::{Streamer, GatewayPayload, GatewayEvent, VoiceEvent};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Channel for outbound gateway opcodes (your WS client sends these)
    let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel::<GatewayPayload>();

    let mut streamer = Streamer::new(
        "YOUR_USER_ID".to_string(),
        gateway_tx,
    );

    // Spawn a task to forward gateway_rx → your WS client
    tokio::spawn(async move {
        while let Some(payload) = gateway_rx.recv().await {
            // Serialize and send to Discord main gateway
            // ws_sender.send(serde_json::to_string(&payload).unwrap()).await;
        }
    });

    // Join a voice channel
    let mut voice_events = streamer.join_voice(
        Some("GUILD_ID".to_string()),
        "CHANNEL_ID".to_string(),
    );

    // Feed VOICE_STATE_UPDATE and VOICE_SERVER_UPDATE from your WS into the streamer
    // streamer.handle_event(GatewayEvent::from_dispatch("VOICE_STATE_UPDATE", data)).await;
    // streamer.handle_event(GatewayEvent::from_dispatch("VOICE_SERVER_UPDATE", data)).await;
}
```

### 2. Handle voice events

```rust
use discord_stream_rs::VoiceEvent;

while let Some(event) = voice_events.recv().await {
    match event {
        VoiceEvent::Ready(params) => {
            println!("WebRTC ready! audio_ssrc={} video_ssrc={}",
                params.audio_ssrc, params.video_ssrc);
            // Now configure WebRtcWrapper and start streaming
        }
        VoiceEvent::SelectProtocolAck { sdp, dave_version } => {
            println!("SDP received, DAVE version={:?}", dave_version);
        }
        VoiceEvent::Resumed => println!("Voice connection resumed"),
        VoiceEvent::MlsCommitWelcome(_) => {} // DAVE internal
    }
}
```

### 3. Stream a video file

```rust
use discord_stream_rs::{
    media::{demux, ContainerFormat, AudioStream, VideoStream},
    voice::webrtc::WebRtcWrapper,
    DaveHandler,
};
use std::sync::Arc;
use tokio::sync::Mutex;

async fn stream_file(
    params: discord_stream_rs::WebRtcParams,
    dave: Arc<Mutex<DaveHandler>>,
) {
    // Open and demux any container (MKV, MP4, etc.)
    let result = demux("video.mkv", ContainerFormat::Matroska)
        .await
        .expect("Failed to open file");

    let webrtc = Arc::new(Mutex::new(WebRtcWrapper::new(dave)));

    // Initialize peer connection
    {
        let mut w = webrtc.lock().await;
        w.init().await.expect("WebRTC init failed");
        w.set_packetizer(&params, "H264").expect("Packetizer error");
    }

    // Spawn audio and video streams
    let (video_info, video_rx) = result.video.unwrap();
    let (audio_info, audio_rx) = result.audio.unwrap();

    let (stop_tx_v, stop_rx_v) = tokio::sync::oneshot::channel();
    let (stop_tx_a, stop_rx_a) = tokio::sync::oneshot::channel();

    let webrtc_v = webrtc.clone();
    let webrtc_a = webrtc.clone();

    tokio::spawn(async move {
        let (mut vs, _) = VideoStream::new(webrtc_v, false);
        vs.run(video_rx, stop_rx_v).await;
    });

    tokio::spawn(async move {
        let (mut aus, _) = AudioStream::new(webrtc_a, false);
        aus.run(audio_rx, stop_rx_a).await;
    });

    // Stop after 30 seconds
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let _ = stop_tx_v.send(());
    let _ = stop_tx_a.send(());
}
```

### 4. Go Live / Screen Share

```rust
// Start a Go Live stream
streamer.create_stream()?;

// Stop it
streamer.stop_stream()?;

// Leave voice
streamer.leave_voice();
```

---

## Codec Support

| Codec | Video | Audio | Notes |
|-------|-------|-------|-------|
| H264  | ✅ | — | SPS VUI auto-patched for real-time |
| H265  | ✅ | — | |
| VP8   | ✅ | — | |
| VP9   | ✅ | — | |
| AV1   | ✅ | — | |
| Opus  | — | ✅ | Only supported audio codec |

---

## Hardware Encoder Settings

### Software (CPU)

```rust
use discord_stream_rs::media::encoders::{SoftwareEncoders, X264Settings, X265Settings};
use discord_stream_rs::media::encoders::software::{X264Tune, X26xPreset};

let encoders = SoftwareEncoders::build(
    Some(X264Settings {
        preset: X26xPreset::Veryfast,
        tune: X264Tune::Zerolatency,
    }),
    None, // x265 defaults
);

// encoders.h264.name  == "libx264"
// encoders.h264.options == ["-forced-idr 1", "-tune zerolatency", "-preset veryfast"]
```

### NVENC (NVIDIA GPU)

```rust
use discord_stream_rs::media::encoders::{NvencEncoders, NvencSettings, NvencPreset};

let encoders = NvencEncoders::build(NvencSettings {
    preset: NvencPreset::P4,
    spatial_aq: true,
    temporal_aq: false,
    gpu: Some(0),
});

// encoders.h264.name == "h264_nvenc"
```

### VA-API (Intel / AMD)

```rust
use discord_stream_rs::media::encoders::{VaapiEncoders, VaapiSettings};

let encoders = VaapiEncoders::build(VaapiSettings {
    device: "/dev/dri/renderD128".to_string(),
});

// encoders.h264.name == "h264_vaapi"
// encoders.h264.global_options == ["-vaapi_device", "/dev/dri/renderD128"]
```

---

## Stream Key Utilities

```rust
use discord_stream_rs::{generate_stream_key, parse_stream_key, StreamType};

// Generate
let key = generate_stream_key(StreamType::Guild, Some("guild_id"), "channel_id", "user_id");
// → "guild:guild_id:channel_id:user_id"

// Parse
let parsed = parse_stream_key("guild:123:456:789").unwrap();
assert_eq!(parsed.guild_id, Some("123".to_string()));
assert_eq!(parsed.channel_id, "456");
```

---

## H264 Processing Utilities

```rust
use discord_stream_rs::processing::{split_nalu, rewrite_sps_vui, H264Helpers, AnnexBHelpers};

// Split Annex-B stream into individual NALUs
let nalus = split_nalu(&h264_frame_bytes);

// Check NALU type
let unit_type = H264Helpers::nal_unit_type(nalus[0]);

// Rewrite SPS VUI (max_num_reorder_frames = 0)
let patched_sps = rewrite_sps_vui(sps_nalu_bytes);
```

---

## Crate Structure

```
discord-stream-rs
├── dave          — DAVE E2EE session wrapper
├── gateway       — Main gateway opcodes, events, Streamer controller
├── voice         — Voice WS state machine, WebRTC wrapper, codec constants
├── processing    — H264 Annex-B bitstream reader/writer, SPS VUI rewriter
└── media         — FFmpeg demuxer, decoder, audio/video streams, encoder settings
```

---

## Credits

- Original TypeScript library: [`@dank074/discord-video-stream`](https://github.com/dank074/discord-video-stream)
- DAVE E2EE Rust crate: [`davey`](https://crates.io/crates/davey) by [@snazzah](https://github.com/snazzah)
- WebRTC: [`webrtc-rs`](https://github.com/webrtc-rs/webrtc)

---

## License

MIT
