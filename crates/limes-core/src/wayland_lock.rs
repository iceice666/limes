use std::sync::{Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
};

use crate::error::{LimesError, Result};
use crate::lock::DisplayBackend;

const LOCK_START_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_LOOP_TICK: Duration = Duration::from_millis(16);

/// Wayland `ext-session-lock-v1` display backend.
///
/// This backend asks the compositor to lock the current Wayland session and
/// keeps a small Wayland event loop alive until `unlock` is called. It only
/// establishes lock surfaces so the configured frontend can provide lock UI.
#[derive(Default)]
pub struct WaylandSessionLockBackend {
    worker: Mutex<Option<LockWorker>>,
}

struct LockWorker {
    unlock_tx: mpsc::Sender<()>,
    handle: JoinHandle<Result<()>>,
}

impl DisplayBackend for WaylandSessionLockBackend {
    fn lock(&self) -> Result<()> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| LimesError::Lock("wayland lock worker mutex poisoned".to_owned()))?;

        if worker.is_some() {
            return Err(LimesError::Lock(
                "wayland session is already locked".to_owned(),
            ));
        }

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (unlock_tx, unlock_rx) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("limes-wayland-session-lock".to_owned())
            .spawn(move || run_lock_worker(ready_tx, unlock_rx))
            .map_err(|error| {
                LimesError::Lock(format!("failed to spawn wayland lock worker: {error}"))
            })?;

        match ready_rx.recv_timeout(LOCK_START_TIMEOUT) {
            Ok(Ok(())) => {
                *worker = Some(LockWorker { unlock_tx, handle });
                Ok(())
            }
            Ok(Err(error)) => {
                let _ = handle.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = unlock_tx.send(());
                let _ = handle.join();
                Err(LimesError::Lock(
                    "timed out waiting for wayland compositor to confirm session lock".to_owned(),
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
                Ok(Err(error)) => Err(error),
                Ok(Ok(())) => Err(LimesError::Lock(
                    "wayland lock worker exited before compositor confirmed lock".to_owned(),
                )),
                Err(_) => Err(LimesError::Lock("wayland lock worker panicked".to_owned())),
            },
        }
    }

    fn unlock(&self) -> Result<()> {
        let worker = self
            .worker
            .lock()
            .map_err(|_| LimesError::Lock("wayland lock worker mutex poisoned".to_owned()))?
            .take()
            .ok_or_else(|| LimesError::Lock("wayland session is not locked".to_owned()))?;

        stop_worker(worker)
    }
}

impl Drop for WaylandSessionLockBackend {
    fn drop(&mut self) {
        let worker = match self.worker.get_mut() {
            Ok(worker) => worker.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(worker) = worker {
            let _ = stop_worker(worker);
        }
    }
}

fn stop_worker(worker: LockWorker) -> Result<()> {
    let _ = worker.unlock_tx.send(());
    match worker.handle.join() {
        Ok(result) => result,
        Err(_) => Err(LimesError::Lock("wayland lock worker panicked".to_owned())),
    }
}

fn run_lock_worker(
    ready_tx: mpsc::SyncSender<Result<()>>,
    unlock_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let conn = Connection::connect_to_env().map_err(|error| {
        LimesError::Lock(format!("failed to connect to wayland compositor: {error}"))
    })?;
    let (globals, event_queue) = registry_queue_init(&conn).map_err(|error| {
        LimesError::Lock(format!("failed to initialize wayland registry: {error}"))
    })?;
    let qh: QueueHandle<AppData> = event_queue.handle();
    let mut event_loop: EventLoop<AppData> = EventLoop::try_new().map_err(|error| {
        LimesError::Lock(format!("failed to initialize wayland event loop: {error}"))
    })?;

    let mut app = AppData {
        conn: conn.clone(),
        compositor_state: CompositorState::bind(&globals, &qh).map_err(|error| {
            LimesError::Lock(format!("wayland compositor interface unavailable: {error}"))
        })?,
        output_state: OutputState::new(&globals, &qh),
        registry_state: RegistryState::new(&globals),
        session_lock_state: SessionLockState::new(&globals, &qh),
        session_lock: None,
        lock_surfaces: Vec::new(),
        ready_tx: Some(ready_tx),
        unlock_rx,
        exit: false,
        result: Ok(()),
    };

    app.session_lock = Some(app.session_lock_state.lock(&qh).map_err(|error| {
        LimesError::Lock(format!(
            "wayland ext-session-lock-v1 is unavailable: {error}"
        ))
    })?);

    WaylandSource::new(conn, event_queue)
        .insert(event_loop.handle())
        .map_err(|error| {
            LimesError::Lock(format!("failed to register wayland event source: {error}"))
        })?;

    let start = Instant::now();
    while !app.exit {
        event_loop
            .dispatch(EVENT_LOOP_TICK, &mut app)
            .map_err(|error| LimesError::Lock(format!("wayland event loop failed: {error}")))?;

        if app.unlock_rx.try_recv().is_ok() {
            app.unlock();
        }

        if app.ready_tx.is_some() && start.elapsed() >= LOCK_START_TIMEOUT {
            app.report_ready(Err(LimesError::Lock(
                "timed out waiting for wayland compositor to confirm session lock".to_owned(),
            )));
            app.exit = true;
        }
    }

    app.result
}

struct AppData {
    conn: Connection,
    compositor_state: CompositorState,
    output_state: OutputState,
    registry_state: RegistryState,

    session_lock_state: SessionLockState,
    session_lock: Option<SessionLock>,
    lock_surfaces: Vec<SessionLockSurface>,
    ready_tx: Option<mpsc::SyncSender<Result<()>>>,
    unlock_rx: mpsc::Receiver<()>,
    exit: bool,
    result: Result<()>,
}

impl AppData {
    fn report_ready(&mut self, result: Result<()>) {
        if let Some(tx) = self.ready_tx.take() {
            let _ = tx.send(result);
        }
    }

    fn unlock(&mut self) {
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
        _qh: &QueueHandle<Self>,
        session_lock_surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
            return;
        }

        // The lock surface is intentionally left visually blank. Frontend windows
        // can provide the user-facing UI while this backend keeps Wayland in a
        // locked state.
        session_lock_surface.wl_surface().commit();
    }
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

smithay_client_toolkit::delegate_compositor!(AppData);
smithay_client_toolkit::delegate_output!(AppData);
smithay_client_toolkit::delegate_session_lock!(AppData);
smithay_client_toolkit::delegate_registry!(AppData);
