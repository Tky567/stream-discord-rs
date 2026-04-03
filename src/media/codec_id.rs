/// FFmpeg codec IDs — mirrors `LibavCodecId.ts` (`AVCodecID` enum).
///
/// Rather than duplicating all 500+ entries, we re-export the canonical type
/// from `ffmpeg_next::codec::id::Id`, which is generated from the same
/// `codec_id.h` header that the TypeScript file was taken from.

pub use ffmpeg_next::codec::id::Id as AVCodecID;
