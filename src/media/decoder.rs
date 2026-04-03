/// Libav decoder — ported from `LibavDecoder.ts`.
///
/// Creates a decoder for a given stream and decodes packets into raw frames.

use crate::media::base_stream::MediaPacket;
use ffmpeg_next::{
    self as ffmpeg,
    codec::{decoder, Context},
    format::stream::Stream,
    frame,
    software::scaling::{self, flag::Flags as ScaleFlags},
    util::format::pixel::Pixel,
};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("Decoder already freed")]
    Freed,
}

/// Decoded video frame.
#[derive(Debug)]
pub struct DecodedFrame {
    /// Raw RGBA pixel data.
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub pts: i64,
}

/// Wraps an ffmpeg decoder + optional pixel format converter (RGBA output).
pub struct VideoDecoder {
    decoder: decoder::Video,
    scaler: Option<scaling::Context>,
    freed: bool,
    width: u32,
    height: u32,
}

impl VideoDecoder {
    /// Build a `VideoDecoder` from a stream. Mirrors `createDecoder()` in
    /// `LibavDecoder.ts`.
    pub fn from_stream(stream: &Stream) -> Result<Self, DecodeError> {
        ffmpeg::init()?;
        let context = Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;
        let width = decoder.width();
        let height = decoder.height();

        // Build a swscale context to convert any pixel format → RGBA
        let scaler = scaling::Context::get(
            decoder.format(),
            width,
            height,
            Pixel::RGBA,
            width,
            height,
            ScaleFlags::BILINEAR,
        )
        .ok();

        Ok(Self {
            decoder,
            scaler,
            freed: false,
            width,
            height,
        })
    }

    /// Decode all frames from a packet and return them as RGBA `DecodedFrame`s.
    pub fn decode_all(&mut self, packet: &ffmpeg::Packet) -> Result<Vec<DecodedFrame>, DecodeError> {
        if self.freed {
            return Err(DecodeError::Freed);
        }

        self.decoder.send_packet(packet)?;
        let mut frames = Vec::new();
        let mut yuv_frame = frame::Video::empty();

        while self.decoder.receive_frame(&mut yuv_frame).is_ok() {
            let pts = yuv_frame.pts().unwrap_or(0);
            let rgba = if let Some(ref mut scaler) = self.scaler {
                let mut rgba_frame = frame::Video::empty();
                scaler.run(&yuv_frame, &mut rgba_frame)?;
                rgba_frame.data(0).to_vec()
            } else {
                yuv_frame.data(0).to_vec()
            };
            frames.push(DecodedFrame {
                data: rgba,
                width: self.width,
                height: self.height,
                pts,
            });
        }
        Ok(frames)
    }

    pub fn free(&mut self) {
        self.freed = true;
        debug!("VideoDecoder freed");
    }
}

// ---------------------------------------------------------------------------
// Audio decoder
// ---------------------------------------------------------------------------

/// Wraps an ffmpeg audio decoder.
pub struct AudioDecoder {
    decoder: decoder::Audio,
    freed: bool,
}

impl AudioDecoder {
    pub fn from_stream(stream: &Stream) -> Result<Self, DecodeError> {
        ffmpeg::init()?;
        let context = Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().audio()?;
        Ok(Self { decoder, freed: false })
    }

    /// Decode all audio frames from a packet.  Returns raw PCM byte data.
    pub fn decode_all(&mut self, packet: &ffmpeg::Packet) -> Result<Vec<Vec<u8>>, DecodeError> {
        if self.freed {
            return Err(DecodeError::Freed);
        }
        self.decoder.send_packet(packet)?;
        let mut result = Vec::new();
        let mut frame = frame::Audio::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            result.push(frame.data(0).to_vec());
        }
        Ok(result)
    }

    pub fn free(&mut self) {
        self.freed = true;
        debug!("AudioDecoder freed");
    }
}
