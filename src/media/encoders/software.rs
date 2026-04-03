/// Encoder settings for the software (CPU) codec backend.
///
/// Mirrors the return type of `software()` in `encoders/software.ts`.

// ---------------------------------------------------------------------------
// x264 / x265 preset
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum X26xPreset {
    Ultrafast,
    Superfast,
    #[default]
    Superfast_,   // alias — matches TS default "superfast"
    Veryfast,
    Faster,
    Fast,
    Medium,
    Slow,
    Slower,
    Veryslow,
    Placebo,
}

impl X26xPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            X26xPreset::Ultrafast => "ultrafast",
            X26xPreset::Superfast | X26xPreset::Superfast_ => "superfast",
            X26xPreset::Veryfast => "veryfast",
            X26xPreset::Faster => "faster",
            X26xPreset::Fast => "fast",
            X26xPreset::Medium => "medium",
            X26xPreset::Slow => "slow",
            X26xPreset::Slower => "slower",
            X26xPreset::Veryslow => "veryslow",
            X26xPreset::Placebo => "placebo",
        }
    }
}

// ---------------------------------------------------------------------------
// x264 settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum X264Tune {
    Film,
    Animation,
    Grain,
    Stillimage,
    Fastdecode,
    Zerolatency,
    Psnr,
    Ssim,
}

impl X264Tune {
    pub fn as_str(&self) -> &'static str {
        match self {
            X264Tune::Film => "film",
            X264Tune::Animation => "animation",
            X264Tune::Grain => "grain",
            X264Tune::Stillimage => "stillimage",
            X264Tune::Fastdecode => "fastdecode",
            X264Tune::Zerolatency => "zerolatency",
            X264Tune::Psnr => "psnr",
            X264Tune::Ssim => "ssim",
        }
    }
}

#[derive(Debug, Clone)]
pub struct X264Settings {
    pub preset: X26xPreset,
    pub tune: X264Tune,
}

impl Default for X264Settings {
    fn default() -> Self {
        Self {
            preset: X26xPreset::Superfast_,
            tune: X264Tune::Film,
        }
    }
}

// ---------------------------------------------------------------------------
// x265 settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum X265Tune {
    Psnr,
    Ssim,
    Grain,
    Fastdecode,
    Zerolatency,
    Animation,
}

impl X265Tune {
    pub fn as_str(&self) -> &'static str {
        match self {
            X265Tune::Psnr => "psnr",
            X265Tune::Ssim => "ssim",
            X265Tune::Grain => "grain",
            X265Tune::Fastdecode => "fastdecode",
            X265Tune::Zerolatency => "zerolatency",
            X265Tune::Animation => "animation",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct X265Settings {
    pub preset: X26xPreset,
    pub tune: Option<X265Tune>,
}

// ---------------------------------------------------------------------------
// Per-codec encoder descriptor
// ---------------------------------------------------------------------------

/// Name + ffmpeg option flags for a single codec encoder.
#[derive(Debug, Clone)]
pub struct EncoderInfo {
    /// FFmpeg codec name (e.g. `"libx264"`).
    pub name: &'static str,
    /// Additional ffmpeg options (e.g. `["-preset superfast", "-tune film"]`).
    pub options: Vec<String>,
}

/// Collection of encoder info for every supported codec.
#[derive(Debug, Clone)]
pub struct SoftwareEncoders {
    pub h264: EncoderInfo,
    pub h265: EncoderInfo,
    pub vp8: EncoderInfo,
    pub vp9: EncoderInfo,
    pub av1: EncoderInfo,
}

impl SoftwareEncoders {
    /// Build with the given x264/x265 settings.  `None` uses defaults.
    pub fn build(x264: Option<X264Settings>, x265: Option<X265Settings>) -> Self {
        let x264 = x264.unwrap_or_default();
        let x265 = x265.unwrap_or_default();

        let mut x265_opts = vec![
            "-forced-idr 1".to_owned(),
            format!("-preset {}", x265.preset.as_str()),
        ];
        if let Some(tune) = &x265.tune {
            x265_opts.push(format!("-tune {}", tune.as_str()));
        }

        Self {
            h264: EncoderInfo {
                name: "libx264",
                options: vec![
                    "-forced-idr 1".to_owned(),
                    format!("-tune {}", x264.tune.as_str()),
                    format!("-preset {}", x264.preset.as_str()),
                ],
            },
            h265: EncoderInfo {
                name: "libx265",
                options: x265_opts,
            },
            vp8: EncoderInfo {
                name: "libvpx",
                options: vec!["-deadline 20000".to_owned()],
            },
            vp9: EncoderInfo {
                name: "libvpx-vp9",
                options: vec!["-deadline 20000".to_owned()],
            },
            av1: EncoderInfo {
                name: "libsvtav1",
                options: vec![],
            },
        }
    }
}

impl Default for SoftwareEncoders {
    fn default() -> Self {
        Self::build(None, None)
    }
}
