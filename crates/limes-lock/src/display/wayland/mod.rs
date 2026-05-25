mod app;
mod worker;

use std::sync::{Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use limes_common::{LimesError, Result};

use crate::display::DisplayBackend;
use crate::display::wayland::worker::run_lock_worker;

pub(super) const LOCK_START_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const EVENT_LOOP_TICK: Duration = Duration::from_millis(16);

/// Wayland `ext-session-lock-v1` display backend.
///
/// This backend asks the compositor to lock the current Wayland session and
/// keeps a small Wayland event loop alive until `unlock` is called. It renders
/// opaque blank lock surfaces; user-facing Wayland lock UIs must render on
/// their own session-lock surfaces instead of normal layer-shell surfaces.
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
