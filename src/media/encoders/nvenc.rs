/// NVENC hardware encoder settings.
///
/// Mirrors `nvenc()` in `encoders/nvenc.ts`.

use super::software::EncoderInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NvencPreset {
    P1,
    P2,
    P3,
    #[default]
    P4,
    P5,
    P6,
    P7,
}

impl NvencPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            NvencPreset::P1 => "p1",
            NvencPreset::P2 => "p2",
            NvencPreset::P3 => "p3",
            NvencPreset::P4 => "p4",
            NvencPreset::P5 => "p5",
            NvencPreset::P6 => "p6",
            NvencPreset::P7 => "p7",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NvencSettings {
    pub preset: NvencPreset,
    pub spatial_aq: bool,
    pub temporal_aq: bool,
    /// GPU index to use, or `None` for default.
    pub gpu: Option<u32>,
}

impl Default for NvencSettings {
    fn default() -> Self {
        Self {
            preset: NvencPreset::P4,
            spatial_aq: false,
            temporal_aq: false,
            gpu: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NvencEncoders {
    pub h264: EncoderInfo,
    pub h265: EncoderInfo,
    pub av1: EncoderInfo,
}

impl NvencEncoders {
    pub fn build(settings: NvencSettings) -> Self {
        let mut opts = vec![
            format!("-preset {}", settings.preset.as_str()),
            format!("-spatial-aq {}", settings.spatial_aq as u8),
            format!("-temporal-aq {}", settings.temporal_aq as u8),
        ];
        if let Some(gpu) = settings.gpu {
            opts.push(format!("-gpu {gpu}"));
        }

        Self {
            h264: EncoderInfo { name: "h264_nvenc", options: opts.clone() },
            h265: EncoderInfo { name: "hevc_nvenc", options: opts.clone() },
            av1:  EncoderInfo { name: "av1_nvenc",  options: opts },
        }
    }
}

impl Default for NvencEncoders {
    fn default() -> Self {
        Self::build(NvencSettings::default())
    }
}
