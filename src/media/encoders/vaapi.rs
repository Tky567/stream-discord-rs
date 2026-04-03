/// VA-API hardware encoder settings.
///
/// Mirrors `vaapi()` in `encoders/vaapi.ts`.

use super::software::EncoderInfo;

#[derive(Debug, Clone)]
pub struct VaapiSettings {
    /// DRM render node, e.g. `"/dev/dri/renderD128"`.
    pub device: String,
}

impl Default for VaapiSettings {
    fn default() -> Self {
        Self {
            device: "/dev/dri/renderD128".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VaapiEncoderInfo {
    pub name: &'static str,
    pub options: Vec<String>,
    /// Extra global ffmpeg options (e.g. `-vaapi_device /dev/dri/renderD128`).
    pub global_options: Vec<String>,
    /// Output filter chain (e.g. `["format=nv12|vaapi", "hwupload"]`).
    pub out_filters: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct VaapiEncoders {
    pub h264: VaapiEncoderInfo,
    pub h265: VaapiEncoderInfo,
    pub av1:  VaapiEncoderInfo,
}

impl VaapiEncoders {
    pub fn build(settings: VaapiSettings) -> Self {
        let global_options = vec!["-vaapi_device".to_owned(), settings.device.clone()];
        let out_filters = vec!["format=nv12|vaapi", "hwupload"];
        let make = |name: &'static str| VaapiEncoderInfo {
            name,
            options: vec![],
            global_options: global_options.clone(),
            out_filters: out_filters.clone(),
        };
        Self {
            h264: make("h264_vaapi"),
            h265: make("hevc_vaapi"),
            av1:  make("av1_vaapi"),
        }
    }
}

impl Default for VaapiEncoders {
    fn default() -> Self {
        Self::build(VaapiSettings::default())
    }
}
