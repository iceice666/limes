use std::ffi::CString;
use std::process::Command;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use limes_proto::{LimesEvent, SessionHandle, SessionSpec};

use crate::error::{LimesError, Result};
use crate::events::EventBus;

pub trait SessionBackend: Send + Sync {
    fn start(&self, spec: &SessionSpec) -> Result<SessionHandle>;
    fn wait(&self, handle: &SessionHandle) -> Result<i32>;
    fn terminate(&self, handle: &SessionHandle) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalSessionBackend;

impl SessionBackend for LocalSessionBackend {
    fn start(&self, spec: &SessionSpec) -> Result<SessionHandle> {
        let (program, args) = spec
            .command
            .split_first()
            .ok_or_else(|| LimesError::Session("session command is empty".to_owned()))?;

        let mut command = Command::new(program);
        command.args(args);
        command.env_clear();
        command.envs(spec.env.iter().map(|(key, value)| (key, value)));
        if let Some(working_dir) = &spec.working_dir {
            command.current_dir(working_dir);
        }

        #[cfg(unix)]
        {
            let username = CString::new(spec.username.clone()).map_err(|error| {
                LimesError::Session(format!("invalid session username for initgroups: {error}"))
            })?;
            let uid = spec.uid;
            let gid = spec.gid;
            unsafe {
                command.pre_exec(move || {
                    if libc::geteuid() != 0 {
                        return Ok(());
                    }
                    if libc::initgroups(username.as_ptr(), gid) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::setgid(gid) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::setuid(uid) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let child = command.spawn().map_err(|error| {
            LimesError::Session(format!("failed to spawn session `{program}`: {error}"))
        })?;

        Ok(SessionHandle {
            pid: child.id(),
            username: spec.username.clone(),
            command: spec.command.clone(),
            auth_session_id: spec.auth_session_id.clone(),
        })
    }

    fn wait(&self, handle: &SessionHandle) -> Result<i32> {
        #[cfg(unix)]
        {
            let mut status = 0;
            let pid = unsafe { libc::waitpid(handle.pid as libc::pid_t, &mut status, 0) };
            if pid < 0 {
                return Err(LimesError::Session(format!(
                    "failed waiting for session pid {}: {}",
                    handle.pid,
                    std::io::Error::last_os_error()
                )));
            }
            Ok(status)
        }

        #[cfg(not(unix))]
        {
            let _ = handle;
            Err(LimesError::Unsupported(
                "session waiting is only implemented on unix".to_owned(),
            ))
        }
    }

    fn terminate(&self, handle: &SessionHandle) -> Result<()> {
        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .arg(handle.pid.to_string())
                .status()
                .map_err(|error| {
                    LimesError::Session(format!(
                        "failed to invoke kill for {}: {error}",
                        handle.pid
                    ))
                })?;
            if !status.success() {
                return Err(LimesError::Session(format!(
                    "kill failed for session pid {} with status {status}",
                    handle.pid
                )));
            }
        }

        #[cfg(not(unix))]
        {
            let _ = handle;
            return Err(LimesError::Unsupported(
                "session termination is only stubbed on unix".to_owned(),
            ));
        }

        Ok(())
    }
}

pub struct SessionManager {
    backend: Arc<dyn SessionBackend>,
    events: EventBus,
}

impl SessionManager {
    #[must_use]
    pub fn new(backend: Arc<dyn SessionBackend>, events: EventBus) -> Self {
        Self { backend, events }
    }

    pub fn start(&self, spec: &SessionSpec) -> Result<SessionHandle> {
        let handle = self.backend.start(spec)?;
        self.events.emit(LimesEvent::SessionStarted {
            username: handle.username.clone(),
            pid: handle.pid,
        });
        Ok(handle)
    }

    pub fn wait(&self, handle: &SessionHandle) -> Result<i32> {
        self.backend.wait(handle)
    }

    pub fn terminate(&self, handle: &SessionHandle) -> Result<()> {
        self.backend.terminate(handle)
    }
}
