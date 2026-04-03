/// AnnexB NAL unit helpers — ported from `AnnexBHelper.ts`.
///
/// Provides H264 / H265 NALU type enums, per-codec helpers (header parsing,
/// AUD detection), and `split_nalu` for demuxing Annex-B streams.

// ---------------------------------------------------------------------------
// H264 NAL unit types
// ---------------------------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264NalUnitType {
    Unspecified = 0,
    CodedSliceNonIdr = 1,
    CodedSlicePartitionA = 2,
    CodedSlicePartitionB = 3,
    CodedSlicePartitionC = 4,
    CodedSliceIdr = 5,
    Sei = 6,
    Sps = 7,
    Pps = 8,
    AccessUnitDelimiter = 9,
    EndOfSequence = 10,
    EndOfStream = 11,
    FillerData = 12,
    SeiExtension = 13,
    PrefixNalUnit = 14,
    SubsetSps = 15,
}

impl H264NalUnitType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Unspecified),
            1 => Some(Self::CodedSliceNonIdr),
            2 => Some(Self::CodedSlicePartitionA),
            3 => Some(Self::CodedSlicePartitionB),
            4 => Some(Self::CodedSlicePartitionC),
            5 => Some(Self::CodedSliceIdr),
            6 => Some(Self::Sei),
            7 => Some(Self::Sps),
            8 => Some(Self::Pps),
            9 => Some(Self::AccessUnitDelimiter),
            10 => Some(Self::EndOfSequence),
            11 => Some(Self::EndOfStream),
            12 => Some(Self::FillerData),
            13 => Some(Self::SeiExtension),
            14 => Some(Self::PrefixNalUnit),
            15 => Some(Self::SubsetSps),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// H265 NAL unit types
// ---------------------------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum H265NalUnitType {
    TRAIL_N = 0,
    TRAIL_R = 1,
    TSA_N = 2,
    TSA_R = 3,
    STSA_N = 4,
    STSA_R = 5,
    RADL_N = 6,
    RADL_R = 7,
    RASL_N = 8,
    RASL_R = 9,
    RSV_VCL_N10 = 10,
    RSV_VCL_R11 = 11,
    RSV_VCL_N12 = 12,
    RSV_VCL_R13 = 13,
    RSV_VCL_N14 = 14,
    RSV_VCL_R15 = 15,
    BLA_W_LP = 16,
    BLA_W_RADL = 17,
    BLA_N_LP = 18,
    IDR_W_RADL = 19,
    IDR_N_LP = 20,
    CRA_NUT = 21,
    RSV_IRAP_VCL22 = 22,
    RSV_IRAP_VCL23 = 23,
    RSV_VCL24 = 24,
    RSV_VCL25 = 25,
    RSV_VCL26 = 26,
    RSV_VCL27 = 27,
    RSV_VCL28 = 28,
    RSV_VCL29 = 29,
    RSV_VCL30 = 30,
    RSV_VCL31 = 31,
    VPS_NUT = 32,
    SPS_NUT = 33,
    PPS_NUT = 34,
    AUD_NUT = 35,
    EOS_NUT = 36,
    EOB_NUT = 37,
    FD_NUT = 38,
    PREFIX_SEI_NUT = 39,
    SUFFIX_SEI_NUT = 40,
    RSV_NVCL41 = 41,
    RSV_NVCL42 = 42,
    RSV_NVCL43 = 43,
    RSV_NVCL44 = 44,
    RSV_NVCL45 = 45,
    RSV_NVCL46 = 46,
    RSV_NVCL47 = 47,
}

impl H265NalUnitType {
    pub fn from_u8(v: u8) -> Option<Self> {
        if v <= 47 {
            // SAFETY: repr(u8) with contiguous values 0-47
            Some(unsafe { std::mem::transmute(v) })
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Codec-agnostic helpers trait + impls
// ---------------------------------------------------------------------------

pub trait AnnexBHelpers {
    /// Extract NAL unit type from the first byte(s) of the NALU payload.
    fn nal_unit_type(frame: &[u8]) -> u8;
    /// Split NALU into (header_bytes, payload_bytes).
    fn split_header(frame: &[u8]) -> (&[u8], &[u8]);
    /// Return `true` if the unit type represents an Access Unit Delimiter.
    fn is_aud(unit_type: u8) -> bool;
}

pub struct H264Helpers;

impl AnnexBHelpers for H264Helpers {
    #[inline]
    fn nal_unit_type(frame: &[u8]) -> u8 {
        frame[0] & 0x1F
    }
    fn split_header(frame: &[u8]) -> (&[u8], &[u8]) {
        frame.split_at(1)
    }
    fn is_aud(unit_type: u8) -> bool {
        unit_type == H264NalUnitType::AccessUnitDelimiter as u8
    }
}

pub struct H265Helpers;

impl AnnexBHelpers for H265Helpers {
    #[inline]
    fn nal_unit_type(frame: &[u8]) -> u8 {
        (frame[0] >> 1) & 0x3F
    }
    fn split_header(frame: &[u8]) -> (&[u8], &[u8]) {
        frame.split_at(2)
    }
    fn is_aud(unit_type: u8) -> bool {
        unit_type == H265NalUnitType::AUD_NUT as u8
    }
}

// ---------------------------------------------------------------------------
// Annex-B start code + NAL unit splitter
// ---------------------------------------------------------------------------

/// 3-byte start code used in Annex-B streams.
pub const START_CODE_3: [u8; 3] = [0, 0, 1];

/// Split an Annex-B buffer into individual NALU byte slices.
///
/// Handles both 3-byte (`0x00 0x00 0x01`) and 4-byte (`0x00 0x00 0x00 0x01`)
/// start codes. Mirrors `splitNalu()` in `AnnexBHelper.ts`.
pub fn split_nalu(buf: &[u8]) -> Vec<&[u8]> {
    let mut nalus = Vec::new();
    let mut remaining = buf;

    while !remaining.is_empty() {
        // Find next `0x00 0x00 0x01`
        let pos = find_start_code(remaining);

        let (nalu, rest, start_len) = match pos {
            None => {
                // No more start codes — the rest is one NALU
                (remaining, &[] as &[u8], 0)
            }
            Some(p) => {
                let length = if p > 0 && remaining[p - 1] == 0 {
                    // 4-byte start code
                    (p - 1, &remaining[p - 1..][4..], 4)
                } else {
                    (p, &remaining[p + 3..], 3)
                };
                (&remaining[..length.0], length.1, length.2)
            }
        };
        let _ = start_len;

        if !nalu.is_empty() {
            nalus.push(nalu);
        }

        if pos.is_none() {
            break;
        }

        // Recalculate without borrow conflict
        let p = pos.unwrap();
        let skip = if p > 0 && remaining[p - 1] == 0 {
            p - 1 + 4
        } else {
            p + 3
        };
        remaining = &remaining[skip..];
    }
    nalus
}

fn find_start_code(buf: &[u8]) -> Option<usize> {
    // Find `0x00 0x00 0x01`
    for i in 0..buf.len().saturating_sub(2) {
        if buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 1 {
            return Some(i);
        }
    }
    None
}
