use std::sync::mpsc;
use std::time::Instant;

use limes_common::{LimesError, Result};
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::{calloop::EventLoop, calloop_wayland_source::WaylandSource},
    registry::RegistryState,
    session_lock::SessionLockState,
    shm::Shm,
};
use wayland_client::{Connection, QueueHandle, globals::registry_queue_init};

use crate::display::wayland::app::AppData;
use crate::display::wayland::{EVENT_LOOP_TICK, LOCK_START_TIMEOUT};

pub(super) fn run_lock_worker(
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
        shm: Shm::bind(&globals, &qh).map_err(|error| {
            LimesError::Lock(format!("wayland shm interface unavailable: {error}"))
        })?,
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
