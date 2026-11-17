//! Linux distribution packaging and sandbox portal diagnostics for Muvrix.
//!
//! Provides runtime identification of package containment (AppImage, Flatpak,
//! Debian .deb, Fedora .rpm, or Native ELF) alongside XDG desktop portal
//! permission inspection and release installer verification.
//!
//! Permission reporting is evidence based. A permission is only reported as
//! `GRANTED` when the sandbox configuration explicitly grants it *and* the
//! corresponding socket or bus is actually reachable. Availability of a socket
//! path is reported as `AVAILABLE`, and unconfined host access is reported as
//! `DIRECT` so that direct access is never mistaken for a portal grant.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Detected or target distribution packaging format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PackageFormat {
    AppImage,
    Flatpak,
    Deb,
    Rpm,
    NativeElf,
}

impl PackageFormat {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AppImage => "AppImage Standalone",
            Self::Flatpak => "Flatpak Sandboxed Bundle",
            Self::Deb => "Debian / Ubuntu (.deb)",
            Self::Rpm => "Fedora / RHEL (.rpm)",
            Self::NativeElf => "Native Host ELF Binary",
        }
    }

    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::AppImage => "APPIMAGE",
            Self::Flatpak => "FLATPAK",
            Self::Deb => "DEBIAN / UBUNTU",
            Self::Rpm => "FEDORA / RPM",
            Self::NativeElf => "NATIVE ELF",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::AppImage => {
                "Zero-install portable single-file executable bundle with embedded dependencies"
            }
            Self::Flatpak => {
                "Isolated user-space sandbox container with portal-mediated desktop integrations"
            }
            Self::Deb => "Native dpkg/APT system package with shared system library linking",
            Self::Rpm => "Native RPM/DNF system package with system-level update lifecycle",
            Self::NativeElf => {
                "Direct local machine compilation executed without packaging wrapper"
            }
        }
    }

    pub fn package_file_name(&self) -> &'static str {
        match self {
            Self::AppImage => "Muvrix-0.1.0-x86_64.AppImage",
            Self::Flatpak => "org.muvrix.Muvrix.flatpak",
            Self::Deb => "muvrix_0.1.0_amd64.deb",
            Self::Rpm => "muvrix-0.1.0-1.x86_64.rpm",
            Self::NativeElf => "muvrix-ui",
        }
    }

    pub fn install_command_hint(&self) -> &'static str {
        match self {
            Self::AppImage => "chmod +x Muvrix-*.AppImage && ./Muvrix-*.AppImage",
            Self::Flatpak => "flatpak install --user org.muvrix.Muvrix.flatpak",
            Self::Deb => "sudo dpkg -i muvrix_0.1.0_amd64.deb || sudo apt install -f",
            Self::Rpm => "sudo dnf install ./muvrix-0.1.0-1.x86_64.rpm",
            Self::NativeElf => "cargo run --release -p muvrix-ui",
        }
    }
}

/// Status of an XDG desktop portal or system interface permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PortalStatus {
    /// The capability is explicitly granted by sandbox policy and was observed
    /// to be reachable.
    Granted,
    /// The resource appears accessible or available, but no explicit grant was
    /// confirmed.
    Available,
    /// Direct host access, rather than portal-mediated access.
    Direct,
    /// Access is explicitly denied by sandbox policy.
    Denied,
    /// The capability is not supported by the host environment.
    Unsupported,
    /// The permission cannot be reliably established from available evidence.
    Unknown,
}

impl PortalStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Granted => "GRANTED",
            Self::Available => "AVAILABLE",
            Self::Direct => "DIRECT",
            Self::Denied => "DENIED",
            Self::Unsupported => "UNSUPPORTED",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Human readable meaning of the status, shown as a badge tooltip.
    pub fn meaning(&self) -> &'static str {
        match self {
            Self::Granted => {
                "Permission explicitly confirmed by sandbox policy and observed reachable"
            }
            Self::Available => "Resource appears available; no explicit permission confirmation",
            Self::Direct => "Direct host access; not mediated by a desktop portal",
            Self::Denied => "Access explicitly denied by sandbox policy",
            Self::Unsupported => "Capability is not supported in this environment",
            Self::Unknown => "Permission cannot be reliably established",
        }
    }
}

/// Observed evidence about a local Unix domain socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SocketState {
    /// No socket path could be resolved from the environment. This is the
    /// default so that absent evidence is never read as reachability.
    #[default]
    Absent,
    /// A socket path exists, but connecting to it failed.
    Present,
    /// A connection to the socket was established.
    Connectable,
}

/// Result of a direct D-Bus probe of the XDG desktop portal service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortalProbe {
    /// The portal answered a property query on the session bus.
    Responded,
    /// The session bus was reachable, but the portal did not answer.
    NoPortal,
    /// No session bus could be reached at all. This is the default: an absent
    /// probe is never treated as evidence of a grant.
    #[default]
    NoBus,
}

/// Parsed `/.flatpak-info` description of the active sandbox.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxInfo {
    pub app_id: Option<String>,
    pub shared: Vec<String>,
    pub sockets: Vec<String>,
    pub devices: Vec<String>,
    pub filesystems: Vec<String>,
    /// `(name, policy)` pairs from the Session Bus Policy section.
    pub bus_policy: Vec<(String, String)>,
}

impl SandboxInfo {
    /// Parses a `/.flatpak-info` file. Unknown sections are ignored.
    pub fn parse(contents: &str) -> Self {
        let mut info = SandboxInfo::default();
        let mut section = String::new();

        for raw in contents.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.to_string();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());

            match section.as_str() {
                "Application" => {
                    if key == "name" && !value.is_empty() {
                        info.app_id = Some(value.to_string());
                    }
                }
                "Context" => {
                    let values: Vec<String> = value
                        .split(';')
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                        .collect();
                    match key {
                        "shared" => info.shared = values,
                        "sockets" => info.sockets = values,
                        "devices" => info.devices = values,
                        "filesystems" => info.filesystems = values,
                        _ => {}
                    }
                }
                "Session Bus Policy" => {
                    info.bus_policy.push((key.to_string(), value.to_string()));
                }
                _ => {}
            }
        }

        info
    }

    fn grants_socket(&self, name: &str) -> bool {
        self.sockets.iter().any(|socket| socket == name)
    }

    fn grants_shared(&self, name: &str) -> bool {
        self.shared.iter().any(|entry| entry == name)
    }

    fn owns_bus_name(&self, name: &str) -> bool {
        self.bus_policy
            .iter()
            .any(|(bus_name, policy)| bus_name == name && policy == "own")
    }
}

/// Evidence collected from the running host or sandbox.
///
/// This is a plain data structure so that permission evaluation can be tested
/// deterministically for both unconfined and sandboxed environments without
/// mutating the process environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEvidence {
    /// Parsed sandbox metadata, present only when running inside Flatpak.
    pub sandbox: Option<SandboxInfo>,
    pub wayland_socket: Option<SocketState>,
    pub x11_socket: Option<SocketState>,
    pub pulse_socket: Option<SocketState>,
    pub pipewire_socket: Option<SocketState>,
    pub session_bus: SocketState,
    pub portal_file_chooser: PortalProbe,
}

fn socket_state(path: Option<PathBuf>) -> Option<SocketState> {
    let path = path?;
    if !path.exists() {
        return Some(SocketState::Absent);
    }
    match std::os::unix::net::UnixStream::connect(&path) {
        Ok(_) => Some(SocketState::Connectable),
        Err(_) => Some(SocketState::Present),
    }
}

fn wayland_socket_path() -> Option<PathBuf> {
    let display = std::env::var("WAYLAND_DISPLAY").ok()?;
    if display.is_empty() {
        return None;
    }
    if display.starts_with('/') {
        return Some(PathBuf::from(display));
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(Path::new(&runtime).join(display))
}

fn x11_socket_path() -> Option<PathBuf> {
    let display = std::env::var("DISPLAY").ok()?;
    // Only a local display has a reachable abstract socket path here; remote
    // and TCP displays carry no local socket evidence.
    if !display.starts_with(':') {
        return None;
    }
    let screen = display.trim_start_matches(':');
    let number = screen.split('.').next()?;
    if number.is_empty() {
        return None;
    }
    Some(PathBuf::from(format!("/tmp/.X11-unix/X{number}")))
}

fn pulse_socket_path() -> Option<PathBuf> {
    if let Ok(server) = std::env::var("PULSE_SERVER") {
        if let Some(path) = server.strip_prefix("unix:") {
            return Some(PathBuf::from(path));
        }
        if server.starts_with('/') {
            return Some(PathBuf::from(server));
        }
        // TCP server specification: no local socket can be inspected.
        return None;
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(Path::new(&runtime).join("pulse/native"))
}

fn pipewire_socket_path() -> Option<PathBuf> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(Path::new(&runtime).join("pipewire-0"))
}

/// Probes the session bus and the XDG desktop portal.
///
/// A successful portal property query is real evidence that the portal service
/// is present and answering on this bus, rather than an inference from the
/// presence of an environment variable.
fn probe_session_bus_and_portal() -> (SocketState, PortalProbe) {
    let connection = zbus::blocking::connection::Builder::session().and_then(|builder| {
        builder
            .method_timeout(std::time::Duration::from_millis(750))
            .build()
    });

    let Ok(connection) = connection else {
        return (SocketState::Absent, PortalProbe::NoBus);
    };

    let reply = connection.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.portal.FileChooser", "version"),
    );

    match reply {
        Ok(_) => (SocketState::Connectable, PortalProbe::Responded),
        Err(_) => (SocketState::Connectable, PortalProbe::NoPortal),
    }
}

impl HostEvidence {
    /// Collects live evidence from the current process environment.
    pub fn probe() -> Self {
        let sandbox = if std::env::var_os("FLATPAK_ID").is_some() {
            fs::read_to_string("/.flatpak-info")
                .ok()
                .map(|contents| SandboxInfo::parse(&contents))
        } else {
            None
        };

        let (session_bus, portal_file_chooser) = probe_session_bus_and_portal();

        Self {
            sandbox,
            wayland_socket: socket_state(wayland_socket_path()),
            x11_socket: socket_state(x11_socket_path()),
            pulse_socket: socket_state(pulse_socket_path()),
            pipewire_socket: socket_state(pipewire_socket_path()),
            session_bus,
            portal_file_chooser,
        }
    }
}

/// Diagnostic descriptor for a sandbox or portal permission.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PortalPermission {
    pub name: String,
    pub category: String,
    pub status: PortalStatus,
    pub details: String,
}

/// Release installer artifact metadata with cryptographic integrity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReleaseInstallerInfo {
    pub format: PackageFormat,
    pub file_name: String,
    pub description: String,
    pub install_command: String,
    pub sha256_hash: String,
    pub verified: bool,
    pub size_bytes: u64,
}

/// Complete packaging and sandbox diagnostic state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackagingInfo {
    pub detected_format: PackageFormat,
    pub sandbox_id: Option<String>,
    pub portal_permissions: Vec<PortalPermission>,
    pub release_installers: Vec<ReleaseInstallerInfo>,
    pub manifest_verified: bool,
    pub verification_message: Option<String>,
}

impl PackagingInfo {
    /// Detects current execution containment and portal permissions.
    pub fn detect() -> Self {
        let mut sandbox_id = None;
        let detected_format = if let Ok(flatpak_id) = std::env::var("FLATPAK_ID") {
            sandbox_id = Some(flatpak_id);
            PackageFormat::Flatpak
        } else if Path::new("/.flatpak-info").exists() {
            sandbox_id = Some("org.muvrix.Muvrix".to_string());
            PackageFormat::Flatpak
        } else if std::env::var("APPIMAGE").is_ok() || std::env::var("APPDIR").is_ok() {
            sandbox_id = std::env::var("APPIMAGE").ok();
            PackageFormat::AppImage
        } else {
            // Check host packaging context
            let exe_path = std::env::current_exe().unwrap_or_default();
            let is_system_bin = exe_path.starts_with("/usr/bin") || exe_path.starts_with("/bin");
            if is_system_bin && Path::new("/etc/debian_version").exists() {
                PackageFormat::Deb
            } else if is_system_bin
                && (Path::new("/etc/fedora-release").exists()
                    || Path::new("/etc/redhat-release").exists())
            {
                PackageFormat::Rpm
            } else {
                PackageFormat::NativeElf
            }
        };

        let evidence = HostEvidence::probe();
        let portal_permissions = Self::evaluate_portal_permissions(detected_format, &evidence);
        let mut info = Self {
            detected_format,
            sandbox_id: sandbox_id.or_else(|| {
                evidence
                    .sandbox
                    .as_ref()
                    .and_then(|sandbox| sandbox.app_id.clone())
            }),
            portal_permissions,
            release_installers: Vec::new(),
            manifest_verified: false,
            verification_message: None,
        };

        info.refresh_release_installers();
        info
    }

    /// Inspects live system portal capabilities and active sandboxing permissions.
    pub fn inspect_portal_permissions(format: PackageFormat) -> Vec<PortalPermission> {
        Self::evaluate_portal_permissions(format, &HostEvidence::probe())
    }

    /// Derives portal permissions strictly from supplied evidence.
    ///
    /// `GRANTED` requires both an explicit sandbox grant and an observed
    /// reachable socket/bus. Unconfined execution reports `DIRECT`, a visible
    /// but unconfirmed resource reports `AVAILABLE`, and missing evidence is
    /// never upgraded to a grant.
    pub fn evaluate_portal_permissions(
        format: PackageFormat,
        evidence: &HostEvidence,
    ) -> Vec<PortalPermission> {
        let mut permissions = Vec::new();
        let sandbox = evidence.sandbox.as_ref();
        let is_flatpak = format == PackageFormat::Flatpak;

        // 1. Wayland / DRM Graphics Pipeline
        let (display_status, display_details) = match sandbox {
            Some(info) => {
                let wayland_granted = info.grants_socket("wayland");
                let x11_granted = info.grants_socket("x11") || info.grants_socket("fallback-x11");
                if wayland_granted && evidence.wayland_socket == Some(SocketState::Connectable) {
                    (
                        PortalStatus::Granted,
                        "Sandbox grants the Wayland socket and it accepts connections".to_string(),
                    )
                } else if wayland_granted {
                    (
                        PortalStatus::Denied,
                        "Sandbox declares Wayland access, but no Wayland socket is reachable"
                            .to_string(),
                    )
                } else if x11_granted && evidence.x11_socket == Some(SocketState::Connectable) {
                    (
                        PortalStatus::Granted,
                        "Sandbox grants the X11 socket and it accepts connections".to_string(),
                    )
                } else if x11_granted {
                    (
                        PortalStatus::Denied,
                        "Sandbox declares X11 access, but no X11 socket is reachable".to_string(),
                    )
                } else {
                    (
                        PortalStatus::Denied,
                        "Sandbox grants neither Wayland nor X11 display sockets".to_string(),
                    )
                }
            }
            None => match (evidence.wayland_socket, evidence.x11_socket) {
                (Some(SocketState::Connectable), _) => (
                    PortalStatus::Direct,
                    "Direct host Wayland socket reachable; display access is not portal mediated"
                        .to_string(),
                ),
                (_, Some(SocketState::Connectable)) => (
                    PortalStatus::Direct,
                    "Direct host X11 socket reachable; display access is not portal mediated"
                        .to_string(),
                ),
                (Some(SocketState::Present), _) | (_, Some(SocketState::Present)) => (
                    PortalStatus::Available,
                    "A display socket path exists but refused a connection; access unconfirmed"
                        .to_string(),
                ),
                _ => (
                    PortalStatus::Unsupported,
                    "No Wayland or X11 display socket could be resolved".to_string(),
                ),
            },
        };
        permissions.push(PortalPermission {
            name: "Wayland / DRM Display Server".to_string(),
            category: "Display & GPU".to_string(),
            status: display_status,
            details: display_details,
        });

        // 2. PulseAudio / PipeWire Audio Bus
        let audio_connectable = matches!(evidence.pulse_socket, Some(SocketState::Connectable))
            || matches!(evidence.pipewire_socket, Some(SocketState::Connectable));
        let audio_present = matches!(evidence.pulse_socket, Some(SocketState::Present))
            || matches!(evidence.pipewire_socket, Some(SocketState::Present));
        let (audio_status, audio_details) = match sandbox {
            Some(info) => {
                let audio_granted = info.grants_socket("pulseaudio");
                if audio_granted && audio_connectable {
                    (
                        PortalStatus::Granted,
                        "Sandbox grants the audio socket and it accepts connections; playback is \
                         still subject to the sound server policy"
                            .to_string(),
                    )
                } else if audio_granted {
                    (
                        PortalStatus::Denied,
                        "Sandbox declares audio access, but no audio socket is reachable"
                            .to_string(),
                    )
                } else if audio_connectable {
                    (
                        PortalStatus::Available,
                        "A PipeWire/PulseAudio socket is reachable, but the sandbox grants no audio \
                         socket; playback authorization is not confirmed"
                            .to_string(),
                    )
                } else if is_flatpak {
                    (
                        PortalStatus::Denied,
                        "Sandbox grants no audio socket and none is reachable".to_string(),
                    )
                } else {
                    (
                        PortalStatus::Unknown,
                        "Sandboxed audio state could not be established".to_string(),
                    )
                }
            }
            None => {
                if audio_connectable {
                    (
                        PortalStatus::Direct,
                        "Direct host PipeWire/PulseAudio socket reachable; application-level audio \
                         authorization is not verified"
                            .to_string(),
                    )
                } else if audio_present {
                    (
                        PortalStatus::Available,
                        "An audio socket path exists but refused a connection; authorization \
                         unconfirmed"
                            .to_string(),
                    )
                } else {
                    (
                        PortalStatus::Unknown,
                        "No PipeWire/PulseAudio socket could be resolved; permission not established"
                            .to_string(),
                    )
                }
            }
        };
        permissions.push(PortalPermission {
            name: "PulseAudio / PipeWire Audio Bus".to_string(),
            category: "Audio DSP".to_string(),
            status: audio_status,
            details: audio_details,
        });

        // 3. Network & Remote Streaming Sockets
        let (net_status, net_details) = match sandbox {
            Some(info) => {
                if info.grants_shared("network") {
                    (
                        PortalStatus::Granted,
                        "Sandbox explicitly shares the host network namespace (--share=network)"
                            .to_string(),
                    )
                } else {
                    (
                        PortalStatus::Denied,
                        "Sandbox policy does not share the host network namespace".to_string(),
                    )
                }
            }
            None => (
                PortalStatus::Direct,
                "Unconfined process uses the host network stack directly; this is not a portal \
                 permission"
                    .to_string(),
            ),
        };
        permissions.push(PortalPermission {
            name: "Network & Streaming Sockets".to_string(),
            category: "Remote Media".to_string(),
            status: net_status,
            details: net_details,
        });

        // 4. XDG Media Folders File Access
        let (file_status, file_details) = match sandbox {
            Some(info) => match evidence.portal_file_chooser {
                PortalProbe::Responded => (
                    PortalStatus::Available,
                    "FileChooser portal responds; individual file access is granted per selection"
                        .to_string(),
                ),
                PortalProbe::NoPortal if info.filesystems.is_empty() => (
                    PortalStatus::Denied,
                    "Sandbox grants no filesystem access and no FileChooser portal answered"
                        .to_string(),
                ),
                PortalProbe::NoPortal => (
                    PortalStatus::Unknown,
                    "Sandbox declares filesystem grants, but no portal response confirmed them"
                        .to_string(),
                ),
                PortalProbe::NoBus => (
                    PortalStatus::Unknown,
                    "Session bus unavailable; portable file access could not be confirmed"
                        .to_string(),
                ),
            },
            None => (
                PortalStatus::Direct,
                format!(
                    "Direct filesystem access to {} and user media directories (unconfined)",
                    std::env::var("XDG_VIDEOS_DIR").unwrap_or_else(|_| "~/Videos".to_string())
                ),
            ),
        };
        permissions.push(PortalPermission {
            name: "XDG Media Folders (Videos & Music)".to_string(),
            category: "Filesystem".to_string(),
            status: file_status,
            details: file_details,
        });

        // 5. D-Bus MPRIS v2 Media Controls
        let (dbus_status, dbus_details) = match sandbox {
            Some(info) => {
                let owns_mpris = info.owns_bus_name("org.mpris.MediaPlayer2.muvrix");
                if evidence.session_bus != SocketState::Connectable {
                    (
                        PortalStatus::Denied,
                        "Session bus is not reachable inside the sandbox".to_string(),
                    )
                } else if owns_mpris {
                    (
                        PortalStatus::Granted,
                        "Sandbox policy grants ownership of org.mpris.MediaPlayer2.muvrix and the \
                         session bus accepts connections"
                            .to_string(),
                    )
                } else {
                    (
                        PortalStatus::Available,
                        "Session bus reachable; sandbox policy does not declare MPRIS name ownership"
                            .to_string(),
                    )
                }
            }
            None => {
                if evidence.session_bus == SocketState::Connectable {
                    (
                        PortalStatus::Direct,
                        "Direct host session D-Bus connection for MPRIS v2 and screen inhibition"
                            .to_string(),
                    )
                } else {
                    (
                        PortalStatus::Unsupported,
                        "No reachable session D-Bus socket in this environment".to_string(),
                    )
                }
            }
        };
        permissions.push(PortalPermission {
            name: "D-Bus MPRIS v2 & Inhibit".to_string(),
            category: "IPC & Hardware Keys".to_string(),
            status: dbus_status,
            details: dbus_details,
        });

        permissions
    }

    /// Resolves the packages directory in both root-run app and cargo test subcrate environments.
    pub fn find_packages_dir() -> PathBuf {
        if Path::new("target/packages").exists() {
            PathBuf::from("target/packages")
        } else if Path::new("../../target/packages").exists() {
            PathBuf::from("../../target/packages")
        } else if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let p = PathBuf::from(manifest_dir);
            if let Some(parent) = p.parent().and_then(|p| p.parent()) {
                let candidate = parent.join("target/packages");
                if candidate.exists() {
                    return candidate;
                }
            }
            PathBuf::from("target/packages")
        } else {
            PathBuf::from("target/packages")
        }
    }

    /// Scans the `target/packages` output directory or populates standard package installer references.
    pub fn refresh_release_installers(&mut self) {
        let repo_target_packages = Self::find_packages_dir();
        let hash_map = Self::load_sha256sums(&repo_target_packages);

        let formats = [
            PackageFormat::AppImage,
            PackageFormat::Flatpak,
            PackageFormat::Deb,
            PackageFormat::Rpm,
        ];

        self.release_installers.clear();
        for fmt in formats {
            let file_name = fmt.package_file_name().to_string();
            let file_path = repo_target_packages.join(&file_name);
            let (sha256_hash, verified, size_bytes) = if file_path.exists() {
                let bytes = fs::read(&file_path).unwrap_or_default();
                let computed = format!("{:x}", Sha256::digest(&bytes));
                let expected = hash_map.get(&file_name).cloned();
                let verified = match expected {
                    Some(ref exp) => exp == &computed,
                    None => true,
                };
                (computed, verified, bytes.len() as u64)
            } else if let Some(stored_hash) = hash_map.get(&file_name) {
                (stored_hash.clone(), true, 0)
            } else {
                ("pending_build_verification".to_string(), false, 0)
            };

            self.release_installers.push(ReleaseInstallerInfo {
                format: fmt,
                file_name,
                description: fmt.description().to_string(),
                install_command: fmt.install_command_hint().to_string(),
                sha256_hash,
                verified,
                size_bytes,
            });
        }
    }

    /// Verifies all release package manifests and SHA256 checksums.
    pub fn verify_checksums(&mut self) -> bool {
        let repo_target_packages = Self::find_packages_dir();
        let sums_path = repo_target_packages.join("SHA256SUMS");

        if !sums_path.exists() {
            self.manifest_verified = false;
            self.verification_message =
                Some("SHA256SUMS file not found in target/packages/".to_string());
            return false;
        }

        let hash_map = Self::load_sha256sums(&repo_target_packages);
        let mut all_ok = true;
        let mut checked_count = 0;

        for installer in &mut self.release_installers {
            let file_path = repo_target_packages.join(&installer.file_name);
            if file_path.exists() {
                if let Ok(bytes) = fs::read(&file_path) {
                    let computed = format!("{:x}", Sha256::digest(&bytes));
                    installer.sha256_hash = computed.clone();
                    installer.size_bytes = bytes.len() as u64;
                    if let Some(expected) = hash_map.get(&installer.file_name) {
                        installer.verified = expected == &computed;
                        if !installer.verified {
                            all_ok = false;
                        }
                    } else {
                        installer.verified = false;
                        all_ok = false;
                    }
                    checked_count += 1;
                }
            } else {
                installer.verified = false;
                all_ok = false;
            }
        }

        self.manifest_verified = all_ok && checked_count > 0;
        self.verification_message = Some(if self.manifest_verified {
            format!(
                "All {} distribution packages verified against SHA256SUMS.",
                checked_count
            )
        } else {
            "Checksum verification failed or packages are missing.".to_string()
        });

        self.manifest_verified
    }

    fn load_sha256sums(dir: &Path) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        let sums_path = dir.join("SHA256SUMS");
        if let Ok(content) = fs::read_to_string(sums_path) {
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let hash = parts[0].to_string();
                    let name = parts[1].trim_start_matches('*').to_string();
                    map.insert(name, hash);
                }
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unconfined_evidence() -> HostEvidence {
        HostEvidence {
            sandbox: None,
            wayland_socket: Some(SocketState::Connectable),
            x11_socket: Some(SocketState::Absent),
            pulse_socket: Some(SocketState::Connectable),
            pipewire_socket: Some(SocketState::Connectable),
            session_bus: SocketState::Connectable,
            portal_file_chooser: PortalProbe::NoPortal,
        }
    }

    fn flatpak_info(shared: &str, sockets: &str, filesystems: &str) -> SandboxInfo {
        SandboxInfo::parse(&format!(
            "[Application]\nname=org.muvrix.Muvrix\n\n[Context]\nshared={shared}\n\
             sockets={sockets}\ndevices=dri;\nfilesystems={filesystems}\n\n\
             [Session Bus Policy]\norg.mpris.MediaPlayer2.muvrix=own\n\
             org.freedesktop.ScreenSaver=talk\n"
        ))
    }

    fn status_of(permissions: &[PortalPermission], needle: &str) -> PortalStatus {
        permissions
            .iter()
            .find(|permission| permission.name.contains(needle))
            .unwrap_or_else(|| panic!("missing permission containing {needle}"))
            .status
    }

    #[test]
    fn test_package_format_properties() {
        let formats = [
            PackageFormat::AppImage,
            PackageFormat::Flatpak,
            PackageFormat::Deb,
            PackageFormat::Rpm,
            PackageFormat::NativeElf,
        ];
        for fmt in formats {
            assert!(!fmt.display_name().is_empty());
            assert!(!fmt.badge_label().is_empty());
            assert!(!fmt.description().is_empty());
            assert!(!fmt.package_file_name().is_empty());
            assert!(!fmt.install_command_hint().is_empty());
        }
    }

    #[test]
    fn test_portal_status_labels_are_distinct() {
        let statuses = [
            PortalStatus::Granted,
            PortalStatus::Available,
            PortalStatus::Direct,
            PortalStatus::Denied,
            PortalStatus::Unsupported,
            PortalStatus::Unknown,
        ];
        let mut labels: Vec<&str> = statuses.iter().map(|s| s.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), 6);
        for status in statuses {
            assert!(!status.meaning().is_empty());
        }
    }

    #[test]
    fn test_unconfined_environment_never_claims_granted() {
        let permissions = PackagingInfo::evaluate_portal_permissions(
            PackageFormat::NativeElf,
            &unconfined_evidence(),
        );

        assert!(
            !permissions
                .iter()
                .any(|p| p.status == PortalStatus::Granted),
            "unconfined host execution must never report a sandbox GRANTED status"
        );
        assert_eq!(status_of(&permissions, "Wayland"), PortalStatus::Direct);
        assert_eq!(status_of(&permissions, "PulseAudio"), PortalStatus::Direct);
        assert_eq!(status_of(&permissions, "Network"), PortalStatus::Direct);
        assert_eq!(status_of(&permissions, "XDG Media"), PortalStatus::Direct);
        assert_eq!(status_of(&permissions, "D-Bus"), PortalStatus::Direct);
    }

    #[test]
    fn test_unconfined_display_access_is_not_a_portal_grant() {
        let evidence = HostEvidence {
            x11_socket: Some(SocketState::Connectable),
            ..unconfined_evidence()
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::NativeElf, &evidence);
        let display = permissions
            .iter()
            .find(|p| p.name.contains("Wayland"))
            .expect("display permission present");
        // Direct host X11 access must not be described as a granted portal permission.
        assert_eq!(display.status, PortalStatus::Direct);
        assert!(display.details.contains("not portal mediated"));
    }

    #[test]
    fn test_runtime_dir_without_sockets_is_unknown_audio() {
        // Regression: a populated XDG_RUNTIME_DIR (or /run/user) is not proof of
        // audio permission. With no reachable socket, audio must be UNKNOWN.
        let evidence = HostEvidence {
            pulse_socket: Some(SocketState::Absent),
            pipewire_socket: Some(SocketState::Absent),
            ..unconfined_evidence()
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::NativeElf, &evidence);
        assert_eq!(status_of(&permissions, "PulseAudio"), PortalStatus::Unknown);
        assert!(!permissions
            .iter()
            .any(|p| p.status == PortalStatus::Granted));
    }

    #[test]
    fn test_audio_socket_availability_is_not_a_grant() {
        // A reachable host audio socket is direct access, not an explicit grant.
        let evidence = HostEvidence {
            pulse_socket: Some(SocketState::Absent),
            pipewire_socket: Some(SocketState::Connectable),
            ..unconfined_evidence()
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::NativeElf, &evidence);
        let audio = permissions
            .iter()
            .find(|p| p.name.contains("PulseAudio"))
            .expect("audio permission present");
        assert_eq!(audio.status, PortalStatus::Direct);
        assert!(audio.details.contains("not verified"));
    }

    #[test]
    fn test_sandboxed_grants_require_reachable_sockets() {
        let evidence = HostEvidence {
            sandbox: Some(flatpak_info(
                "network;ipc;",
                "x11;wayland;fallback-x11;pulseaudio;",
                "xdg-videos:ro;xdg-music:ro;",
            )),
            wayland_socket: Some(SocketState::Connectable),
            x11_socket: Some(SocketState::Absent),
            pulse_socket: Some(SocketState::Connectable),
            pipewire_socket: Some(SocketState::Connectable),
            session_bus: SocketState::Connectable,
            portal_file_chooser: PortalProbe::Responded,
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::Flatpak, &evidence);

        assert_eq!(status_of(&permissions, "Wayland"), PortalStatus::Granted);
        assert_eq!(status_of(&permissions, "PulseAudio"), PortalStatus::Granted);
        assert_eq!(status_of(&permissions, "Network"), PortalStatus::Granted);
        assert_eq!(
            status_of(&permissions, "XDG Media"),
            PortalStatus::Available
        );
        assert_eq!(status_of(&permissions, "D-Bus"), PortalStatus::Granted);
    }

    #[test]
    fn test_sandboxed_declared_grant_without_socket_is_denied() {
        let evidence = HostEvidence {
            sandbox: Some(flatpak_info(
                "network;",
                "wayland;pulseaudio;",
                "xdg-videos:ro;",
            )),
            wayland_socket: Some(SocketState::Absent),
            x11_socket: Some(SocketState::Absent),
            pulse_socket: Some(SocketState::Absent),
            pipewire_socket: Some(SocketState::Absent),
            session_bus: SocketState::Absent,
            portal_file_chooser: PortalProbe::NoBus,
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::Flatpak, &evidence);

        assert_eq!(status_of(&permissions, "Wayland"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "PulseAudio"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "D-Bus"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "XDG Media"), PortalStatus::Unknown);
        // Network was explicitly shared and needs no socket to be granted.
        assert_eq!(status_of(&permissions, "Network"), PortalStatus::Granted);
    }

    #[test]
    fn test_restricted_sandbox_reports_denied_not_granted() {
        let evidence = HostEvidence {
            sandbox: Some(flatpak_info("", "", "")),
            wayland_socket: Some(SocketState::Absent),
            x11_socket: Some(SocketState::Absent),
            pulse_socket: Some(SocketState::Absent),
            pipewire_socket: Some(SocketState::Absent),
            session_bus: SocketState::Absent,
            portal_file_chooser: PortalProbe::NoPortal,
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::Flatpak, &evidence);

        assert_eq!(status_of(&permissions, "Wayland"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "PulseAudio"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "Network"), PortalStatus::Denied);
        assert_eq!(status_of(&permissions, "XDG Media"), PortalStatus::Denied);
        assert!(!permissions
            .iter()
            .any(|p| p.status == PortalStatus::Granted));
    }

    #[test]
    fn test_sandboxed_pipewire_without_audio_grant_is_available() {
        // Regression: a reachable PipeWire socket inside a sandbox that grants no
        // audio socket must be reported AVAILABLE, never GRANTED.
        let evidence = HostEvidence {
            sandbox: Some(flatpak_info("network;", "wayland;", "xdg-music:ro;")),
            wayland_socket: Some(SocketState::Connectable),
            x11_socket: Some(SocketState::Absent),
            pulse_socket: Some(SocketState::Absent),
            pipewire_socket: Some(SocketState::Connectable),
            session_bus: SocketState::Connectable,
            portal_file_chooser: PortalProbe::Responded,
        };
        let permissions =
            PackagingInfo::evaluate_portal_permissions(PackageFormat::Flatpak, &evidence);
        assert_eq!(
            status_of(&permissions, "PulseAudio"),
            PortalStatus::Available
        );
        assert_eq!(status_of(&permissions, "Network"), PortalStatus::Granted);
    }

    #[test]
    fn test_sandbox_metadata_parsing() {
        let parsed = flatpak_info(
            "network;ipc;",
            "wayland;pulseaudio;",
            "xdg-videos:ro;xdg-music:ro;",
        );
        assert_eq!(parsed.app_id.as_deref(), Some("org.muvrix.Muvrix"));
        assert!(parsed.grants_shared("network"));
        assert!(parsed.grants_shared("ipc"));
        assert!(!parsed.grants_shared("host"));
        assert!(parsed.grants_socket("wayland"));
        assert!(parsed.grants_socket("pulseaudio"));
        assert_eq!(parsed.filesystems.len(), 2);
        assert!(parsed.owns_bus_name("org.mpris.MediaPlayer2.muvrix"));
        assert!(!parsed.owns_bus_name("org.freedesktop.ScreenSaver"));
    }

    #[test]
    fn test_packaging_info_detection() {
        let info = PackagingInfo::detect();
        assert!(!info.portal_permissions.is_empty());
        assert_eq!(info.release_installers.len(), 4);
    }

    #[test]
    fn test_portal_permissions_inspection() {
        let permissions = PackagingInfo::inspect_portal_permissions(PackageFormat::NativeElf);
        assert!(!permissions.is_empty());
        assert!(permissions.iter().any(|p| p.name.contains("Wayland")));
        assert!(permissions
            .iter()
            .any(|p| p.name.contains("PulseAudio") || p.name.contains("PipeWire")));
        assert!(permissions.iter().any(|p| p.name.contains("Network")));
    }
}
