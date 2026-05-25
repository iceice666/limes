use std::sync::mpsc;

use limes_common::{LimesError, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shm::{Shm, ShmHandler, raw::RawPool},
};
use wayland_client::{
    Connection, QueueHandle,
    protocol::{wl_buffer, wl_output, wl_shm, wl_surface},
};

pub(super) struct AppData {
    pub(super) conn: Connection,
    pub(super) compositor_state: CompositorState,
    pub(super) output_state: OutputState,
    pub(super) registry_state: RegistryState,
    pub(super) shm: Shm,
    pub(super) session_lock_state: SessionLockState,
    pub(super) session_lock: Option<SessionLock>,
    pub(super) lock_surfaces: Vec<SessionLockSurface>,
    pub(super) ready_tx: Option<mpsc::SyncSender<Result<()>>>,
    pub(super) unlock_rx: mpsc::Receiver<()>,
    pub(super) exit: bool,
    pub(super) result: Result<()>,
}

impl AppData {
    pub(super) fn report_ready(&mut self, result: Result<()>) {
        if let Some(tx) = self.ready_tx.take() {
            let _ = tx.send(result);
        }
    }

    pub(super) fn unlock(&mut self) {
        if let Some(session_lock) = self.session_lock.take() {
            session_lock.unlock();
            let _ = self.conn.roundtrip();
        }
        self.exit = true;
    }
}

impl SessionLockHandler for AppData {
    fn locked(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, session_lock: SessionLock) {
        for output in self.output_state.outputs() {
            let surface = self.compositor_state.create_surface(qh);
            let lock_surface = session_lock.create_lock_surface(surface, &output, qh);
            self.lock_surfaces.push(lock_surface);
        }
        self.report_ready(Ok(()));
    }

    fn finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _session_lock: SessionLock,
    ) {
        self.result = Err(LimesError::Lock(
            "wayland compositor finished the session lock unexpectedly".to_owned(),
        ));
        self.report_ready(self.result.clone());
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        session_lock_surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
            return;
        }

        if let Err(error) = commit_blank_lock_surface(
            &self.shm,
            &session_lock_surface,
            configure.new_size.0,
            configure.new_size.1,
            qh,
        ) {
            self.result = Err(error.clone());
            self.report_ready(Err(error));
            self.exit = true;
        }
    }
}

fn commit_blank_lock_surface(
    shm: &Shm,
    session_lock_surface: &SessionLockSurface,
    width: u32,
    height: u32,
    qh: &QueueHandle<AppData>,
) -> Result<()> {
    let stride = width
        .checked_mul(4)
        .ok_or_else(|| LimesError::Lock("wayland lock surface stride overflow".to_owned()))?;
    let len = stride
        .checked_mul(height)
        .ok_or_else(|| LimesError::Lock("wayland lock surface buffer size overflow".to_owned()))?;
    let width_i32 = i32::try_from(width)
        .map_err(|_| LimesError::Lock("wayland lock surface width is too large".to_owned()))?;
    let height_i32 = i32::try_from(height)
        .map_err(|_| LimesError::Lock("wayland lock surface height is too large".to_owned()))?;
    let stride_i32 = i32::try_from(stride)
        .map_err(|_| LimesError::Lock("wayland lock surface stride is too large".to_owned()))?;
    let len = usize::try_from(len)
        .map_err(|_| LimesError::Lock("wayland lock surface buffer is too large".to_owned()))?;

    let mut pool = RawPool::new(len, shm).map_err(|error| {
        LimesError::Lock(format!(
            "failed to allocate wayland lock surface buffer: {error}"
        ))
    })?;

    let black = 0xFF00_0000_u32.to_le_bytes();
    for chunk in pool.mmap().chunks_exact_mut(4) {
        chunk.copy_from_slice(&black);
    }

    let buffer = pool.create_buffer(
        0,
        width_i32,
        height_i32,
        stride_i32,
        wl_shm::Format::Argb8888,
        (),
        qh,
    );
    session_lock_surface
        .wl_surface()
        .attach(Some(&buffer), 0, 0);
    session_lock_surface.wl_surface().commit();
    buffer.destroy();

    Ok(())
}

impl CompositorHandler for AppData {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for AppData {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl ProvidesRegistryState for AppData {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState,];
}

impl ShmHandler for AppData {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

smithay_client_toolkit::delegate_compositor!(AppData);
smithay_client_toolkit::delegate_output!(AppData);
smithay_client_toolkit::delegate_session_lock!(AppData);
smithay_client_toolkit::delegate_shm!(AppData);
smithay_client_toolkit::delegate_registry!(AppData);
wayland_client::delegate_noop!(AppData: ignore wl_buffer::WlBuffer);
