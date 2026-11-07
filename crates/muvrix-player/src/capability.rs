//! Runtime capability detection and hardware telemetry abstraction.

use muvrix_core::platform::PlatformInfo;

use crate::error::MpvError;
use crate::handle::MpvHandle;
use crate::version::ApiVersion;

/// Memory path used by the video decoder to deliver frames to the renderer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HwdecMemoryPath {
    /// Pure software decoding on CPU directly into host memory.
    Software,
    /// Hardware decoding into GPU video surfaces copied back to host RAM before texture upload.
    /// (e.g. `vaapi-copy`, `d3d11va-copy`, `videotoolbox-copy`).
    CopyBack,
    /// Direct GPU surface/texture sharing without host RAM copy-back.
    /// (e.g. EGL dmabuf import, D3D11 direct render target).
    ZeroCopy,
    /// Hardware decoding inactive or engine idle.
    Inactive,
}

impl HwdecMemoryPath {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Software => "Software (Host RAM)",
            Self::CopyBack => "Copy-Back (VRAM -> Host Staging -> OpenGL Texture)",
            Self::ZeroCopy => "Zero-Copy (Direct Surface/Texture Sharing)",
            Self::Inactive => "Inactive",
        }
    }

    pub fn is_zero_copy(&self) -> bool {
        matches!(self, Self::ZeroCopy)
    }
}

/// Status of hardware acceleration for the active stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwdecStatus {
    /// Active hardware decoding with identified memory path.
    Active {
        decoder: String,
        memory_path: HwdecMemoryPath,
    },
    /// Hardware acceleration requested but fallen back to software decoding.
    SoftwareFallback {
        requested: String,
        active_decoder: String,
        reason: String,
    },
    /// Pure software decoding explicitly configured.
    Disabled,
    /// Engine idle or no file loaded.
    Idle,
}

/// HDR color pipeline and presentation capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HdrPresentationMode {
    /// Standard dynamic range display (SDR).
    SdrOnly,
    /// HDR stream detected; tone-mapped to SDR by engine/libplacebo.
    ToneMappedToSdr { transfer: String, primaries: String },
    /// Full HDR passthrough to HDR display (requires HDR compositor + 10-bit swapchain).
    NativeHdrPassthrough,
}

/// Real-time playback stream diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackDiagnostics {
    pub hwdec_status: HwdecStatus,
    pub video_codec: String,
    pub audio_codec: String,
    pub current_ao: String,
    pub hdr_mode: HdrPresentationMode,
    pub container_fps: f64,
    pub estimated_vf_fps: f64,
}

/// Static and runtime engine capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCapabilities {
    pub platform: PlatformInfo,
    pub api_version: ApiVersion,
    pub client_name: String,
    pub mpv_version: String,
    pub ffmpeg_version: String,
}

impl MpvHandle {
    /// Gathers static and runtime capabilities of the linked libmpv engine and platform.
    pub fn detect_engine_capabilities(&self) -> EngineCapabilities {
        let platform = PlatformInfo::detect();
        let api_version = ApiVersion::current();
        let client_name = self.client_name().unwrap_or_default();
        let mpv_version = self
            .get_property_string("mpv-version")
            .unwrap_or_else(|_| String::from("unknown"));
        let ffmpeg_version = self
            .get_property_string("ffmpeg-version")
            .unwrap_or_else(|_| String::from("unknown"));

        EngineCapabilities {
            platform,
            api_version,
            client_name,
            mpv_version,
            ffmpeg_version,
        }
    }

    /// Evaluates current hardware decoding telemetry and memory paths.
    pub fn query_playback_diagnostics(
        &self,
        requested_hwdec: &str,
    ) -> Result<PlaybackDiagnostics, MpvError> {
        let hwdec_current = self
            .get_property_string("hwdec-current")
            .unwrap_or_default();
        let video_codec = self.get_property_string("video-format").unwrap_or_default();
        let audio_codec = self
            .get_property_string("audio-codec-name")
            .unwrap_or_default();
        let current_ao = self.get_property_string("current-ao").unwrap_or_default();
        let container_fps: f64 = self.get_property("container-fps").unwrap_or(0.0);
        let estimated_vf_fps: f64 = self.get_property("estimated-vf-fps").unwrap_or(0.0);

        let hwdec_status = if video_codec.is_empty() {
            HwdecStatus::Idle
        } else if hwdec_current.is_empty() || hwdec_current == "no" {
            if requested_hwdec.is_empty() || requested_hwdec == "no" {
                HwdecStatus::Disabled
            } else {
                HwdecStatus::SoftwareFallback {
                    requested: requested_hwdec.to_string(),
                    active_decoder: video_codec.clone(),
                    reason: String::from(
                        "Hardware decoder unsupported by GPU or failed to initialize; falling back to software decoder",
                    ),
                }
            }
        } else if hwdec_current.ends_with("-copy") || hwdec_current == "vaapi-copy" {
            HwdecStatus::Active {
                decoder: hwdec_current,
                memory_path: HwdecMemoryPath::CopyBack,
            }
        } else if hwdec_current == "vaapi" {
            // Direct VAAPI without copy suffix
            HwdecStatus::Active {
                decoder: hwdec_current,
                memory_path: HwdecMemoryPath::ZeroCopy,
            }
        } else {
            HwdecStatus::Active {
                decoder: hwdec_current,
                memory_path: HwdecMemoryPath::CopyBack,
            }
        };

        // HDR evaluation
        let gamma = self
            .get_property_string("video-params/gamma")
            .unwrap_or_default();
        let primaries = self
            .get_property_string("video-params/primaries")
            .unwrap_or_default();

        let hdr_mode = if gamma == "pq" || gamma == "hlg" || primaries == "bt.2020" {
            // Note: Standard OpenGL offscreen FBO renders into 8-bit sRGB framebuffer,
            // tone-mapped by mpv/libplacebo. Native HDR requires 10-bit swapchain + compositor HDR protocol.
            HdrPresentationMode::ToneMappedToSdr {
                transfer: gamma,
                primaries,
            }
        } else {
            HdrPresentationMode::SdrOnly
        };

        Ok(PlaybackDiagnostics {
            hwdec_status,
            video_codec,
            audio_codec,
            current_ao,
            hdr_mode,
            container_fps,
            estimated_vf_fps,
        })
    }
}
