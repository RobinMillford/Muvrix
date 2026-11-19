//! VA-API capability probe.
//!
//! The engine is the authority on the decoder, but it cannot explain a decoder
//! it never got. VA-API initialisation happens inside libavcodec, and when it
//! fails the only visible symptom is "hardware requested, software selected".
//! This module asks the same question the decoder asks — can this process drive
//! a VA-API device, and with which client library and driver — *before* a file
//! is opened, so the answer can be logged as one line and used to explain a
//! fallback instead of being guessed from the engine's request string.
//!
//! It deliberately uses `dlopen`/`dlsym` rather than linking libva: that is the
//! only way to report *which* libva the process actually resolved, which is the
//! difference between a working decoder and a version mismatch. A driver built
//! for a newer VA-API minor version exports exactly one `__vaDriverInit_1_N`
//! symbol, so an older client library can never load it; this module makes that
//! mismatch reportable instead of silent.
//!
//! Linux only. On other platforms [`probe`] returns a "not applicable" record.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

/// `VA_STATUS_SUCCESS`.
const VA_STATUS_SUCCESS: c_int = 0;

/// How the process reached (or failed to reach) a VA-API display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaapiState {
    /// A display initialised and a vendor string was returned.
    Ready,
    /// libva loaded but initialisation failed (no driver, ABI mismatch, no GPU).
    InitFailed,
    /// libva itself could not be loaded, or no DRM render node exists.
    Unavailable,
    /// Not a Linux host, or not a platform that uses VA-API.
    NotApplicable,
}

/// Result of one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaapiCapability {
    pub state: VaapiState,
    /// Absolute path of the `libva.so.2` the process resolved.
    pub libva_path: Option<String>,
    /// Driver ABI version reported by a successful `vaInitialize`.
    pub api_version: Option<(c_int, c_int)>,
    /// Vendor/driver string (e.g. `Intel iHD driver for Intel(R) Gen Graphics`).
    pub vendor: Option<String>,
    /// `LIBVA_DRIVER_NAME` actually in effect, when set.
    pub driver_name: Option<String>,
    /// `LIBVA_DRIVERS_PATH` actually in effect, when set.
    pub drivers_path: Option<String>,
    /// DRM render node the probe used (e.g. `/dev/dri/renderD128`).
    pub render_node: Option<String>,
    /// Human-readable failure reason when the state is not `Ready`.
    pub error: Option<String>,
}

impl VaapiCapability {
    /// A probe record that did not apply to this platform.
    pub fn not_applicable(reason: impl Into<String>) -> Self {
        Self {
            state: VaapiState::NotApplicable,
            libva_path: None,
            api_version: None,
            vendor: None,
            driver_name: std::env::var("LIBVA_DRIVER_NAME").ok(),
            drivers_path: std::env::var("LIBVA_DRIVERS_PATH").ok(),
            render_node: None,
            error: Some(reason.into()),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.state == VaapiState::Ready
    }

    /// One-line summary for the startup log.
    pub fn describe(&self) -> String {
        match self.state {
            VaapiState::Ready => format!(
                "VA-API ready: libva={} api={}.{} driver='{}' driver_name={} node={}",
                self.libva_path.as_deref().unwrap_or("<unknown>"),
                self.api_version.map(|(maj, _)| maj).unwrap_or(0),
                self.api_version.map(|(_, min)| min).unwrap_or(0),
                self.vendor.as_deref().unwrap_or("<unknown>"),
                self.driver_name.as_deref().unwrap_or("auto"),
                self.render_node.as_deref().unwrap_or("<none>"),
            ),
            VaapiState::InitFailed | VaapiState::Unavailable => format!(
                "VA-API unavailable: {} (libva={} driver_name={} drivers_path={} node={})",
                self.error.as_deref().unwrap_or("unknown reason"),
                self.libva_path.as_deref().unwrap_or("<not loaded>"),
                self.driver_name.as_deref().unwrap_or("auto"),
                self.drivers_path.as_deref().unwrap_or("<libva default>"),
                self.render_node.as_deref().unwrap_or("<none>"),
            ),
            VaapiState::NotApplicable => format!(
                "VA-API not applicable: {}",
                self.error.as_deref().unwrap_or("non-Linux host")
            ),
        }
    }

    /// Reason to attach to a software fallback, or `None` when VA-API was
    /// never expected to work here.
    pub fn fallback_reason(&self) -> Option<String> {
        match self.state {
            VaapiState::Ready => None,
            VaapiState::InitFailed | VaapiState::Unavailable => Some(
                self.error
                    .clone()
                    .unwrap_or_else(|| "VA-API initialisation failed".to_string()),
            ),
            VaapiState::NotApplicable => None,
        }
    }
}

// --- libva entry points, resolved dynamically ---------------------------------

type VaGetDisplayDrm = unsafe extern "C" fn(c_int) -> *mut c_void;
type VaInitialize = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_int;
type VaQueryVendorString = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type VaTerminate = unsafe extern "C" fn(*mut c_void) -> c_int;

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 2;
const RTLD_GLOBAL: c_int = 0x100;

/// NUL-terminated symbol name for `dlsym`.
fn symbol(name: &str) -> CString {
    CString::new(name).expect("symbol names are static and contain no NUL")
}

fn dl_error() -> String {
    unsafe {
        let err = dlerror();
        if err.is_null() {
            "unknown dlopen error".to_string()
        } else {
            CStr::from_ptr(err).to_string_lossy().into_owned()
        }
    }
}

/// Returns the path a resolved symbol lives in, without needing `Dl_info`.
///
/// Reads `/proc/self/maps` for the containing mapping: cheap, exact, and
/// available on every Linux this application supports.
fn library_path_for(symbol: *mut c_void) -> Option<String> {
    if symbol.is_null() {
        return None;
    }
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    let needle = format!("{symbol:p}");
    // Symbol addresses live inside a mapping; match the nearest preceding
    // mapping start rather than an exact address.
    let target = usize::from_str_radix(needle.trim_start_matches("0x"), 16).ok()?;
    let mut best: Option<(usize, String)> = None;
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let range = parts.next()?;
        let path = parts.last()?;
        if !path.starts_with('/') {
            continue;
        }
        let mut bounds = range.split('-');
        let start = usize::from_str_radix(bounds.next()?, 16).ok()?;
        let end = usize::from_str_radix(bounds.next()?, 16).ok()?;
        if target >= start && target < end {
            return Some(path.to_string());
        }
        if start <= target && best.as_ref().map(|(s, _)| start > *s).unwrap_or(true) {
            best = Some((start, path.to_string()));
        }
    }
    best.map(|(_, path)| path)
}

/// First DRM render node, in stable order.
fn first_render_node() -> Option<String> {
    let mut nodes: Vec<String> = std::fs::read_dir("/dev/dri")
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("renderD"))
                .unwrap_or(false)
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    nodes.sort();
    nodes.into_iter().next()
}

/// Environment the decoder will see for VA-API driver discovery.
fn env_pairs() -> (Option<String>, Option<String>) {
    (
        std::env::var("LIBVA_DRIVER_NAME").ok(),
        std::env::var("LIBVA_DRIVERS_PATH").ok(),
    )
}

/// Probes VA-API with the process's current environment.
pub fn probe() -> VaapiCapability {
    if !cfg!(target_os = "linux") {
        return VaapiCapability::not_applicable("VA-API is a Linux interface");
    }

    let (driver_name, drivers_path) = env_pairs();
    let mut record = VaapiCapability {
        state: VaapiState::Unavailable,
        libva_path: None,
        api_version: None,
        vendor: None,
        driver_name: driver_name.clone(),
        drivers_path: drivers_path.clone(),
        render_node: None,
        error: None,
    };

    let Some(node) = first_render_node() else {
        record.error =
            Some("no DRM render node under /dev/dri (device not passed through?)".into());
        return record;
    };
    record.render_node = Some(node.clone());

    let Some(drm_lib) = CString::new("libva-drm.so.2").ok() else {
        record.error = Some("invalid library name".into());
        return record;
    };
    let Some(va_lib) = CString::new("libva.so.2").ok() else {
        record.error = Some("invalid library name".into());
        return record;
    };

    unsafe {
        // `libva-drm` supplies the DRM display constructor; loading it first also
        // pins libva itself, so both entry points come from one client library.
        let drm = dlopen(drm_lib.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
        if drm.is_null() {
            record.error = Some(format!(
                "libva-drm.so.2 could not be loaded: {}",
                dl_error()
            ));
            return record;
        }
        let va = dlopen(va_lib.as_ptr(), RTLD_NOW | RTLD_GLOBAL);
        if va.is_null() {
            let err = dl_error();
            dlclose(drm);
            record.error = Some(format!("libva.so.2 could not be loaded: {err}"));
            return record;
        }

        let get_display: Option<VaGetDisplayDrm> =
            std::mem::transmute::<*mut c_void, Option<VaGetDisplayDrm>>(dlsym(
                drm,
                symbol("vaGetDisplayDRM").as_ptr(),
            ));
        let initialize: Option<VaInitialize> =
            std::mem::transmute::<*mut c_void, Option<VaInitialize>>(dlsym(
                va,
                symbol("vaInitialize").as_ptr(),
            ));
        let vendor_string: Option<VaQueryVendorString> =
            std::mem::transmute::<*mut c_void, Option<VaQueryVendorString>>(dlsym(
                va,
                symbol("vaQueryVendorString").as_ptr(),
            ));
        let terminate: Option<VaTerminate> = std::mem::transmute::<*mut c_void, Option<VaTerminate>>(
            dlsym(va, symbol("vaTerminate").as_ptr()),
        );

        record.libva_path = library_path_for(dlsym(va, symbol("vaInitialize").as_ptr()));

        let (Some(get_display), Some(initialize)) = (get_display, initialize) else {
            record.error = Some("libva is missing vaGetDisplayDRM/vaInitialize".into());
            dlclose(va);
            dlclose(drm);
            return record;
        };

        let fd = open_rw(&node);
        if fd < 0 {
            record.error = Some(format!("{node} could not be opened for read/write"));
            dlclose(va);
            dlclose(drm);
            return record;
        }

        let display = get_display(fd);
        if display.is_null() {
            record.error = Some(format!("vaGetDisplayDRM({node}) returned no display"));
            libc_close(fd);
            dlclose(va);
            dlclose(drm);
            return record;
        }

        let mut major: c_int = 0;
        let mut minor: c_int = 0;
        let status = initialize(display, &mut major, &mut minor);
        if status == VA_STATUS_SUCCESS {
            record.state = VaapiState::Ready;
            record.api_version = Some((major, minor));
            if let Some(vendor_string) = vendor_string {
                let ptr = vendor_string(display);
                if !ptr.is_null() {
                    record.vendor = Some(CStr::from_ptr(ptr).to_string_lossy().into_owned());
                }
            }
            if let Some(terminate) = terminate {
                terminate(display);
            }
        } else {
            record.state = VaapiState::InitFailed;
            record.error = Some(format!(
                "vaInitialize failed with status {status} on {node}; the loaded libva cannot \
                 initialise the installed VA driver (a client/driver ABI mismatch is the usual \
                 cause, e.g. an older libva shadowing the system one)"
            ));
        }

        libc_close(fd);
        dlclose(va);
        dlclose(drm);
    }

    record
}

extern "C" {
    #[link_name = "open"]
    fn libc_open(path: *const c_char, flags: c_int, ...) -> c_int;
    #[link_name = "close"]
    fn libc_close(fd: c_int) -> c_int;
}

const O_RDWR: c_int = 2;

/// Opens a device node read/write.
fn open_rw(path: &str) -> c_int {
    match CString::new(path) {
        Ok(path) => unsafe { libc_open(path.as_ptr(), O_RDWR) },
        Err(_) => -1,
    }
}

/// Selects a VA-API driver name when the platform has an Intel GPU and the
/// operator has not chosen one.
///
/// **Why this is not a hardcoded default:** libva's own driver selection is
/// correct for every vendor; it only needs help when the legacy `i965` driver is
/// installed next to `iHD` on Gen12+ hardware, where `i965` cannot initialise.
/// The choice is therefore made only for an Intel renderer, only when the user
/// has not set `LIBVA_DRIVER_NAME`, and only when `iHD` is actually present.
/// Returns the driver name it set, if any.
pub fn select_intel_driver_if_needed(renderer: &str) -> Option<&'static str> {
    if std::env::var_os("LIBVA_DRIVER_NAME").is_some() {
        return None;
    }
    let renderer = renderer.to_ascii_lowercase();
    if !renderer.contains("intel") && !renderer.contains("mesa intel") {
        return None;
    }
    let driver_present = driver_search_dirs().iter().any(|dir| {
        dir.join("iHD_drv_video.so").is_file()
            || dir
                .join("intel-vaapi-driver")
                .join("iHD_drv_video.so")
                .is_file()
    });
    if !driver_present {
        return None;
    }
    std::env::set_var("LIBVA_DRIVER_NAME", "iHD");
    Some("iHD")
}

/// Directories libva searches for `*_drv_video.so`, in the same order libva
/// uses: `LIBVA_DRIVERS_PATH`, then its compiled-in default.
fn driver_search_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(path) = std::env::var("LIBVA_DRIVERS_PATH") {
        for entry in path.split(':').filter(|entry| !entry.is_empty()) {
            dirs.push(Path::new(entry).to_path_buf());
        }
    }
    for default in [
        "/usr/lib/x86_64-linux-gnu/dri",
        "/usr/lib64/dri",
        "/usr/lib/dri",
        "/usr/local/lib/dri",
        "/app/lib/dri",
    ] {
        let path = Path::new(default);
        if path.is_dir() {
            dirs.push(path.to_path_buf());
        }
    }
    dirs
}

/// Full preparation sequence used by the application before the engine is
/// created: pick the Intel driver when needed, then probe once and log.
pub fn prepare(renderer: &str) -> VaapiCapability {
    if let Some(driver) = select_intel_driver_if_needed(renderer) {
        log::info!(
            "VA-API driver name selected: '{driver}' (Intel renderer, LIBVA_DRIVER_NAME was unset)"
        );
    }
    let capability = probe();
    match capability.state {
        VaapiState::Ready => log::info!("{}", capability.describe()),
        VaapiState::NotApplicable => log::debug!("{}", capability.describe()),
        _ => log::warn!("{}", capability.describe()),
    }
    capability
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_applicable_never_reports_a_fallback_reason() {
        let record = VaapiCapability::not_applicable("non-Linux host");
        assert_eq!(record.state, VaapiState::NotApplicable);
        assert!(!record.is_ready());
        assert!(record.fallback_reason().is_none());
    }

    #[test]
    fn init_failure_reports_the_error_verbatim() {
        let record = VaapiCapability {
            state: VaapiState::InitFailed,
            libva_path: Some("/usr/lib/libva.so.2".into()),
            api_version: None,
            vendor: None,
            driver_name: Some("iHD".into()),
            drivers_path: None,
            render_node: Some("/dev/dri/renderD128".into()),
            error: Some("vaInitialize failed with status 1".into()),
        };
        assert_eq!(
            record.fallback_reason().as_deref(),
            Some("vaInitialize failed with status 1")
        );
        let line = record.describe();
        assert!(line.contains("VA-API unavailable"));
        assert!(line.contains("/usr/lib/libva.so.2"));
        assert!(line.contains("renderD128"));
    }

    #[test]
    fn ready_record_describes_library_driver_and_node() {
        let record = VaapiCapability {
            state: VaapiState::Ready,
            libva_path: Some("/usr/lib/x86_64-linux-gnu/libva.so.2".into()),
            api_version: Some((1, 22)),
            vendor: Some("Intel iHD driver".into()),
            driver_name: Some("iHD".into()),
            drivers_path: None,
            render_node: Some("/dev/dri/renderD128".into()),
            error: None,
        };
        assert!(record.is_ready());
        assert!(record.fallback_reason().is_none());
        let line = record.describe();
        assert!(line.contains("api=1.22"));
        assert!(line.contains("Intel iHD driver"));
    }

    #[test]
    fn probe_on_this_host_reports_a_state() {
        // The probe must never panic, whatever the host exposes; the concrete
        // state depends on the machine running the test.
        let record = probe();
        assert!(matches!(
            record.state,
            VaapiState::Ready
                | VaapiState::InitFailed
                | VaapiState::Unavailable
                | VaapiState::NotApplicable
        ));
        assert!(!record.describe().is_empty());
    }
}
