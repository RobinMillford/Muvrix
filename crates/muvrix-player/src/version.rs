//! Runtime version queries and acceptance validation.

use crate::error::MpvError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
    pub raw: u64,
}

impl ApiVersion {
    /// Queries the runtime API version directly from the linked `libmpv` binary.
    pub fn current() -> Self {
        let raw = unsafe { libmpv_sys::mpv_client_api_version() } as u64;
        let major = (raw >> 16) as u32;
        let minor = (raw & 0xFFFF) as u32;
        Self { major, minor, raw }
    }

    /// Creates an `ApiVersion` from major and minor version numbers.
    pub const fn new(major: u32, minor: u32) -> Self {
        let raw = ((major as u64) << 16) | (minor as u64);
        Self { major, minor, raw }
    }

    /// Validates whether the runtime version supports the OpenGL render backend.
    ///
    /// Architectural Requirement: `MPV_CLIENT_API_VERSION >= 2.0`.
    pub fn is_gl_compatible(&self) -> bool {
        (self.major, self.minor) >= (2, 0)
    }

    /// Validates whether the runtime version supports the Direct3D 11 render backend.
    ///
    /// Architectural Requirement: `MPV_CLIENT_API_VERSION >= 2.6` (introduced in mpv 0.42.0).
    pub fn is_d3d11_compatible(&self) -> bool {
        (self.major, self.minor) >= (2, 6)
    }

    /// Asserts that the runtime libmpv meets the OpenGL render backend acceptance criteria.
    pub fn validate_gl_prerequisites(&self) -> Result<(), MpvError> {
        if self.is_gl_compatible() {
            Ok(())
        } else {
            Err(MpvError::IncompatibleApiVersion {
                required: ">= 2.0".to_string(),
                found: format!("{}.{}", self.major, self.minor),
            })
        }
    }

    /// Asserts that the runtime libmpv meets the Direct3D 11 render backend acceptance criteria.
    pub fn validate_d3d11_prerequisites(&self) -> Result<(), MpvError> {
        if self.is_d3d11_compatible() {
            Ok(())
        } else {
            Err(MpvError::IncompatibleApiVersion {
                required: ">= 2.6".to_string(),
                found: format!("{}.{}", self.major, self.minor),
            })
        }
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{} (raw: {})", self.major, self.minor, self.raw)
    }
}
