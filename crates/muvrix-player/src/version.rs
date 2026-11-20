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

#[cfg(test)]
mod tests {
    use super::*;

    /// The oldest runtime MUVRIX accepts for the OpenGL render backend.
    const MIN_GL: ApiVersion = ApiVersion::new(2, 0);

    #[test]
    fn minimum_supported_api_is_accepted() {
        assert_eq!(MIN_GL.validate_gl_prerequisites(), Ok(()));
        assert!(MIN_GL.is_gl_compatible());
    }

    #[test]
    fn older_api_is_rejected_with_both_versions_reported() {
        let found = ApiVersion::new(1, 27);
        let err = found.validate_gl_prerequisites().unwrap_err();
        match err {
            MpvError::IncompatibleApiVersion { required, found } => {
                // The message must name what was required and what was seen, so
                // a user can tell an old distro package from a wrong one.
                assert_eq!(required, ">= 2.0");
                assert_eq!(found, "1.27");
            }
            other => panic!("expected IncompatibleApiVersion, got {other:?}"),
        }
        assert!(!found.is_gl_compatible());
    }

    #[test]
    fn newer_api_is_not_rejected_for_being_newer() {
        // A distribution shipping something newer than the development version
        // must keep working; the floor is a minimum, not an equality.
        for v in [(2, 1), (2, 6), (2, 7), (3, 0), (9, 4)] {
            let version = ApiVersion::new(v.0, v.1);
            assert_eq!(
                version.validate_gl_prerequisites(),
                Ok(()),
                "{v:?} should satisfy the GL floor"
            );
        }
    }

    #[test]
    fn optional_d3d11_capability_is_absent_on_older_runtimes() {
        // API 2.0 is enough to run; D3D11 is an optional capability reported as
        // unavailable rather than a startup failure. Linux never uses it.
        let v = ApiVersion::new(2, 0);
        assert_eq!(v.validate_gl_prerequisites(), Ok(()));
        assert!(!v.is_d3d11_compatible());
        assert!(v.validate_d3d11_prerequisites().is_err());
    }

    #[test]
    fn version_round_trips_through_the_raw_encoding() {
        // (major << 16) | minor is the layout mpv_client_api_version() returns.
        let v = ApiVersion::new(2, 6);
        assert_eq!(v.raw, (2 << 16) | 6);
        assert_eq!(ApiVersion::new(2, 6), v);
        assert_eq!(MIN_GL.raw, 2 << 16);
    }

    #[test]
    fn ordering_compares_major_then_minor() {
        assert!(ApiVersion::new(2, 6) > ApiVersion::new(2, 5));
        assert!(ApiVersion::new(3, 0) > ApiVersion::new(2, 99));
        assert!(ApiVersion::new(1, 99) < ApiVersion::new(2, 0));
    }
}
