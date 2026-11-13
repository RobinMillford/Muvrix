//! `wp_color_manager_v1` integration for the Muvrix presentation runner.
//!
//! The runner shares winit's Wayland connection: it never opens a second one
//! (a surface can only be described by objects on the connection that owns it)
//! and it never reads the socket itself. `Connection` is built from the foreign
//! `wl_display*` with its own event queue, so dispatching here can only touch
//! colour-management objects; winit keeps ownership of window/input events.
//!
//! # What "accepted" means
//!
//! A successful `set_image_description` plus a `ready`/`ready2` event proves the
//! *compositor* accepted the description for this surface. It proves nothing
//! about the panel: that separation is carried through the runner's HDR state
//! and its diagnostics.

use std::ffi::c_void;

use wayland_client::globals::{registry_queue_init, GlobalList};
use wayland_client::protocol::wl_registry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1::{self as cm_surface_iface, WpColorManagementSurfaceV1},
    wp_color_manager_v1::{
        self, Feature, Primaries, RenderIntent, TransferFunction, WpColorManagerV1,
    },
    wp_image_description_creator_params_v1::{
        self as creator_params, WpImageDescriptionCreatorParamsV1,
    },
    wp_image_description_v1::{self, Cause, WpImageDescriptionV1},
};

/// Protocol state accumulated from the manager's advertised events.
#[derive(Debug, Default, Clone)]
pub struct ColorCapabilities {
    pub manager_version: u32,
    pub features: Vec<u32>,
    pub transfer_functions: Vec<u32>,
    pub primaries: Vec<u32>,
    pub done: bool,
}

impl ColorCapabilities {
    pub fn has_feature(&self, feature: Feature) -> bool {
        let code = feature as u32;
        self.features.contains(&code)
    }

    pub fn has_tf(&self, tf: TransferFunction) -> bool {
        self.transfer_functions.contains(&(tf as u32))
    }

    pub fn has_primaries(&self, p: Primaries) -> bool {
        self.primaries.contains(&(p as u32))
    }
}

/// State object used for the colour-management event queue.
#[derive(Debug, Default)]
pub struct CmState {
    pub caps: ColorCapabilities,
    /// Identity reported by `ready`/`ready2` for the description we created.
    pub ready_identity: Option<u64>,
    /// Failure reported by `failed` for the description we created.
    pub failure: Option<String>,
    pub error: Option<String>,
}

/// Owns the shared connection's colour-management queue and manager object.
pub struct ColorManager {
    conn: Connection,
    queue: EventQueue<CmState>,
    qh: QueueHandle<CmState>,
    manager: Option<WpColorManagerV1>,
    state: CmState,
    globals: GlobalList,
}

impl std::fmt::Debug for ColorManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorManager")
            .field("caps", &self.state.caps)
            .field("global_error", &self.state.error)
            .finish()
    }
}

impl ColorManager {
    /// Attaches to the live `wl_display` owned by winit.
    ///
    /// # Safety
    ///
    /// `display` must be the `wl_display*` of the connection owning the window,
    /// and must outlive this object. The returned manager creates a private
    /// event queue on that connection; it never reads from the socket.
    pub unsafe fn attach(display: *mut c_void) -> Result<Self, String> {
        if display.is_null() {
            return Err("null wl_display".into());
        }
        let backend = wayland_client::backend::Backend::from_foreign_display(
            display as *mut wayland_sys::client::wl_display,
        );
        let conn = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<CmState>(&conn)
            .map_err(|e| format!("colour-management registry init failed: {e}"))?;
        let qh = queue.handle();

        let mut manager = None;
        let mut caps = ColorCapabilities::default();
        if let Ok(bound) = globals.bind::<WpColorManagerV1, _, _>(&qh, 1..=3, ()) {
            caps.manager_version = bound.version();
            manager = Some(bound);
        }

        let mut this = Self {
            conn,
            queue,
            qh,
            manager,
            state: CmState::default(),
            globals,
        };

        // Drain the manager's initial announcement (features, transfer
        // functions, primaries). Bounded: startup-only, and the compositor
        // sends `done` immediately after the initial burst.
        if this.manager.is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
            while !this.state.caps.done && std::time::Instant::now() < deadline {
                this.roundtrip()?;
            }
            this.state.caps.manager_version = this
                .manager
                .as_ref()
                .map(|m| m.version())
                .unwrap_or_default();
        }

        log::info!(
            "wp_color_manager_v1: v{} features={:?} tf={:?} primaries={:?} done={}",
            this.state.caps.manager_version,
            this.state.caps.features,
            this.state.caps.transfer_functions,
            this.state.caps.primaries,
            this.state.caps.done
        );

        Ok(this)
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| format!("colour-management roundtrip failed: {e}"))?;
        if let Some(err) = self.conn.protocol_error() {
            return Err(format!("compositor protocol error: {err}"));
        }
        Ok(())
    }

    /// Pumps queued colour-management events without reading the socket.
    ///
    /// winit performs the reads; this only dispatches events already decoded
    /// into this connection's queue.
    pub fn dispatch_pending(&mut self) {
        if let Err(e) = self.queue.dispatch_pending(&mut self.state) {
            log::warn!("colour-management dispatch failed: {e}");
        }
    }

    pub fn capabilities(&self) -> &ColorCapabilities {
        &self.state.caps
    }

    pub fn manager_version(&self) -> Option<u32> {
        self.manager.as_ref().map(|m| m.version())
    }

    pub fn supports_pq_bt2020(&self) -> bool {
        self.manager.is_some()
            && self.state.caps.has_tf(TransferFunction::St2084Pq)
            && self.state.caps.has_primaries(Primaries::Bt2020)
    }

    /// Creates a PQ / BT.2020 parametric description and binds it to `surface`.
    pub fn describe_pq_bt2020(
        &mut self,
        surface_ptr: *mut c_void,
        max_nits: u32,
        reference_nits: u32,
    ) -> Result<SurfaceDescription, String> {
        let manager = self
            .manager
            .clone()
            .ok_or("wp_color_manager_v1 is not advertised by this compositor")?;

        // Rebuild a proxy for the surface winit owns. The id is derived from the
        // live `wl_surface*` and validated against the interface by libwayland.
        let id = unsafe {
            wayland_client::backend::ObjectId::from_ptr(
                WlSurface::interface(),
                surface_ptr as *mut wayland_sys::client::wl_proxy,
            )
        }
        .map_err(|e| format!("cannot adopt wl_surface: {e}"))?;
        let surface = WlSurface::from_id(&self.conn, id)
            .map_err(|e| format!("cannot reference wl_surface: {e}"))?;

        let creator: WpImageDescriptionCreatorParamsV1 =
            manager.create_parametric_creator(&self.qh, ());
        creator.set_tf_named(TransferFunction::St2084Pq);
        creator.set_primaries_named(Primaries::Bt2020);
        if self.state.caps.has_feature(Feature::SetLuminances) {
            creator.set_luminances(0, max_nits, reference_nits);
        } else {
            log::warn!(
                "compositor does not advertise set_luminances; PQ description will use \
                 compositor defaults (target peak {max_nits} nits not communicated)"
            );
        }

        self.state.ready_identity = None;
        self.state.failure = None;
        let desc: WpImageDescriptionV1 = creator.create(&self.qh, ());

        // The protocol creates descriptions asynchronously: wait for
        // `ready`/`ready2` (or `failed`) before using it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while self.state.ready_identity.is_none()
            && self.state.failure.is_none()
            && std::time::Instant::now() < deadline
        {
            self.roundtrip()?;
        }

        if let Some(reason) = self.state.failure.take() {
            desc.destroy();
            return Err(format!(
                "compositor rejected PQ/BT.2020 description: {reason}"
            ));
        }
        let identity = self
            .state
            .ready_identity
            .ok_or("compositor never sent ready for the PQ/BT.2020 description")?;

        let cm_surface: WpColorManagementSurfaceV1 = manager.get_surface(&surface, &self.qh, ());
        cm_surface.set_image_description(&desc, RenderIntent::Perceptual);
        self.roundtrip()?;

        log::info!("PQ/BT.2020 image description attached: identity={identity}");

        Ok(SurfaceDescription {
            cm_surface,
            desc,
            identity,
            surface_attached: true,
        })
    }

    /// Binds the plain sRGB description (used when HDR is refused/disabled).
    pub fn describe_srgb(
        &mut self,
        surface_ptr: *mut c_void,
    ) -> Result<SurfaceDescription, String> {
        let manager = self
            .manager
            .clone()
            .ok_or("wp_color_manager_v1 is not advertised by this compositor")?;

        let id = unsafe {
            wayland_client::backend::ObjectId::from_ptr(
                WlSurface::interface(),
                surface_ptr as *mut wayland_sys::client::wl_proxy,
            )
        }
        .map_err(|e| format!("cannot adopt wl_surface: {e}"))?;
        let surface = WlSurface::from_id(&self.conn, id)
            .map_err(|e| format!("cannot reference wl_surface: {e}"))?;

        let creator: WpImageDescriptionCreatorParamsV1 =
            manager.create_parametric_creator(&self.qh, ());
        creator.set_tf_named(TransferFunction::Srgb);
        creator.set_primaries_named(Primaries::Srgb);
        self.state.ready_identity = None;
        self.state.failure = None;
        let desc: WpImageDescriptionV1 = creator.create(&self.qh, ());

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while self.state.ready_identity.is_none()
            && self.state.failure.is_none()
            && std::time::Instant::now() < deadline
        {
            self.roundtrip()?;
        }
        if let Some(reason) = self.state.failure.take() {
            desc.destroy();
            return Err(format!("compositor rejected sRGB description: {reason}"));
        }
        let identity = self
            .state
            .ready_identity
            .ok_or("compositor never sent ready for the sRGB description")?;

        let cm_surface: WpColorManagementSurfaceV1 = manager.get_surface(&surface, &self.qh, ());
        cm_surface.set_image_description(&desc, RenderIntent::Perceptual);
        self.roundtrip()?;

        Ok(SurfaceDescription {
            cm_surface,
            desc,
            identity,
            surface_attached: true,
        })
    }

    /// Number of globals the shared registry exposed (diagnostics only).
    pub fn global_count(&self) -> usize {
        self.globals.contents().clone_list().len()
    }
}

/// A live image description bound to a surface.
pub struct SurfaceDescription {
    cm_surface: WpColorManagementSurfaceV1,
    desc: WpImageDescriptionV1,
    identity: u64,
    surface_attached: bool,
}

impl SurfaceDescription {
    pub fn identity(&self) -> u64 {
        self.identity
    }

    /// Detaches the description but keeps the objects alive for destruction.
    pub fn unset(&mut self) {
        if self.surface_attached {
            self.cm_surface.unset_image_description();
            self.surface_attached = false;
        }
    }
}

impl Drop for SurfaceDescription {
    fn drop(&mut self) {
        if self.surface_attached {
            self.cm_surface.unset_image_description();
        }
        self.cm_surface.destroy();
        self.desc.destroy();
    }
}

impl Drop for ColorManager {
    fn drop(&mut self) {
        // Protocol objects are destroyed by their owners; the manager is last.
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
        let _ = self.conn.flush();
    }
}

// --- dispatch plumbing -------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for CmState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Globals are tracked by `GlobalList` itself.
    }
}

impl Dispatch<WpColorManagerV1, ()> for CmState {
    fn event(
        state: &mut Self,
        _proxy: &WpColorManagerV1,
        event: wp_color_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wp_color_manager_v1::Event;
        match event {
            Event::SupportedFeature { feature } => {
                if let wayland_client::WEnum::Value(v) = feature {
                    state.caps.features.push(v as u32);
                }
            }
            Event::SupportedTfNamed { tf } => {
                if let wayland_client::WEnum::Value(v) = tf {
                    state.caps.transfer_functions.push(v as u32);
                }
            }
            Event::SupportedPrimariesNamed { primaries } => {
                if let wayland_client::WEnum::Value(v) = primaries {
                    state.caps.primaries.push(v as u32);
                }
            }
            Event::Done => state.caps.done = true,
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionCreatorParamsV1, ()> for CmState {
    fn event(
        _state: &mut Self,
        _proxy: &WpImageDescriptionCreatorParamsV1,
        _event: creator_params::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The creator interface defines no events.
    }
}

impl Dispatch<WpColorManagementSurfaceV1, ()> for CmState {
    fn event(
        _state: &mut Self,
        _proxy: &WpColorManagementSurfaceV1,
        _event: cm_surface_iface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // The surface interface defines no events.
    }
}

impl Dispatch<WpImageDescriptionV1, ()> for CmState {
    fn event(
        state: &mut Self,
        _proxy: &WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wp_image_description_v1::Event;
        match event {
            Event::Ready { identity } => state.ready_identity = Some(identity as u64),
            Event::Ready2 {
                identity_hi,
                identity_lo,
            } => {
                state.ready_identity = Some(((identity_hi as u64) << 32) | identity_lo as u64);
            }
            Event::Failed { cause, msg } => {
                let cause = match cause {
                    wayland_client::WEnum::Value(Cause::LowVersion) => "low_version",
                    wayland_client::WEnum::Value(Cause::Unsupported) => "unsupported",
                    wayland_client::WEnum::Value(Cause::OperatingSystem) => "operating_system",
                    wayland_client::WEnum::Value(Cause::NoOutput) => "no_output",
                    _ => "unknown",
                };
                state.failure = Some(format!("{cause}: {msg}"));
            }
            _ => {}
        }
    }
}
