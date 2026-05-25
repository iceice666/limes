use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess};

use crate::auth::pam::conversation::{PamConversation, SecretCString, pam_conversation};
use crate::auth::pam::raw as pam;
use crate::auth::pam::session::PamSession;
use crate::auth::pam::status::{internal_failure, map_auth_status};
use crate::auth::pam::user::{UserRecord, lookup_user};
use crate::auth::traits::{AuthBackend, LockAuthBackend};
use crate::events::EventBus;
use crate::{LimesError, Result};

pub const PAM_SERVICE: &str = "limes";

pub struct PamAuth {
    service: &'static str,
    sessions: Mutex<HashMap<String, PamSession>>,
    next_session_id: AtomicU64,
    events: Option<EventBus>,
}

pub struct PamLockAuth {
    service: &'static str,
    events: Option<EventBus>,
}

impl std::fmt::Debug for PamAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PamAuth")
            .field("service", &self.service)
            .field("sessions", &self.sessions)
            .field("next_session_id", &self.next_session_id)
            .field("events", &self.events.as_ref().map(|_| "<event-bus>"))
            .finish()
    }
}

impl std::fmt::Debug for PamLockAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PamLockAuth")
            .field("service", &self.service)
            .field("events", &self.events.as_ref().map(|_| "<event-bus>"))
            .finish()
    }
}

impl PamAuth {
    #[must_use]
    pub fn new() -> Self {
        Self::with_events(None)
    }

    #[must_use]
    pub fn with_events(events: Option<EventBus>) -> Self {
        Self {
            service: PAM_SERVICE,
            sessions: Mutex::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
            events,
        }
    }

    /// Drops authenticated PAM transactions that have not been opened as login
    /// sessions yet. This gives every new auth challenge a fresh PAM handle
    /// without tearing down already-opened sessions that must later be closed
    /// through `close_session`.
    fn cleanup_pending_auth_state(&self) -> Result<()> {
        let pending = {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|_| LimesError::Auth("PAM session map mutex poisoned".to_owned()))?;
            let pending_ids = sessions
                .iter()
                .filter(|(_, session)| !session.is_open())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();

            pending_ids
                .into_iter()
                .filter_map(|id| sessions.remove(&id))
                .collect::<Vec<_>>()
        };

        drop(pending);
        Ok(())
    }
}

impl PamLockAuth {
    #[must_use]
    pub fn new() -> Self {
        Self::with_events(None)
    }

    #[must_use]
    pub fn with_events(events: Option<EventBus>) -> Self {
        Self {
            service: PAM_SERVICE,
            events,
        }
    }
}

impl AuthBackend for PamAuth {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        self.cleanup_pending_auth_state()
            .map_err(internal_failure)?;

        let (mut new_session, user) = authenticate_pam(self.service, request, self.events.clone())?;
        new_session.establish_credentials()?;

        let session_id = format!(
            "pam-{}",
            self.next_session_id.fetch_add(1, Ordering::Relaxed)
        );
        let success = AuthSuccess {
            username: user.username,
            uid: user.uid,
            gid: user.gid,
            home: user.home,
            shell: user.shell,
            auth_session_id: Some(session_id.clone()),
        };

        let mut sessions = match self.sessions.lock() {
            Ok(sessions) => sessions,
            Err(_) => {
                drop(new_session);
                return Err(AuthFailure::Internal(
                    "PAM session map mutex poisoned".to_owned(),
                ));
            }
        };
        sessions.insert(session_id, new_session);

        Ok(success)
    }

    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| LimesError::Auth("PAM session map mutex poisoned".to_owned()))?;
        let session_id = user.auth_session_id.as_deref().ok_or_else(|| {
            LimesError::Auth(format!(
                "no PAM session id is available for {}",
                user.username
            ))
        })?;
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            LimesError::Auth(format!(
                "no authenticated PAM transaction is available for {}",
                user.username
            ))
        })?;

        session.open()?;
        Ok(session.env())
    }

    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()> {
        let Some(auth_session_id) = auth_session_id else {
            return Ok(());
        };
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| LimesError::Auth("PAM session map mutex poisoned".to_owned()))?;
        drop(sessions.remove(auth_session_id));
        Ok(())
    }
}

impl LockAuthBackend for PamLockAuth {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        let (_session, user) = authenticate_pam(self.service, request, self.events.clone())?;

        Ok(AuthSuccess {
            username: user.username,
            uid: user.uid,
            gid: user.gid,
            home: user.home,
            shell: user.shell,
            auth_session_id: None,
        })
    }

    fn close_session(&self, _auth_session_id: Option<&str>) -> Result<()> {
        Ok(())
    }
}

fn authenticate_pam(
    service: &'static str,
    request: &AuthRequest,
    events: Option<EventBus>,
) -> std::result::Result<(PamSession, UserRecord), AuthFailure> {
    let service = CString::new(service).map_err(internal_failure)?;
    let username = CString::new(request.username.as_str()).map_err(internal_failure)?;
    let password = SecretCString::new(request.password.as_str()).map_err(internal_failure)?;
    let tty = request
        .tty
        .as_deref()
        .filter(|tty| !tty.trim().is_empty())
        .map(CString::new)
        .transpose()
        .map_err(internal_failure)?;

    let mut conversation = Box::new(PamConversation::new(
        username,
        request.username.clone(),
        Some(password),
        events,
    ));
    let conv = pam::PamConv {
        conv: Some(pam_conversation),
        appdata_ptr: (&mut *conversation as *mut PamConversation).cast(),
    };

    let mut handle = ptr::null_mut();
    let mut status = unsafe {
        pam::pam_start(
            service.as_ptr(),
            conversation.username_ptr(),
            &conv,
            &mut handle,
        )
    };
    if status != pam::PAM_SUCCESS {
        return Err(map_auth_status(status, handle));
    }

    if let Some(tty) = &tty {
        status = unsafe { pam::pam_set_item(handle, pam::PAM_TTY, tty.as_ptr().cast()) };
        if status != pam::PAM_SUCCESS {
            let failure = map_auth_status(status, handle);
            unsafe { pam::pam_end(handle, status) };
            return Err(failure);
        }
    }

    status = unsafe { pam::pam_authenticate(handle, 0) };
    if status != pam::PAM_SUCCESS {
        let failure = map_auth_status(status, handle);
        unsafe { pam::pam_end(handle, status) };
        return Err(failure);
    }

    status = unsafe { pam::pam_acct_mgmt(handle, 0) };
    if status != pam::PAM_SUCCESS {
        let failure = map_auth_status(status, handle);
        unsafe { pam::pam_end(handle, status) };
        return Err(failure);
    }

    let Some(user) = lookup_user(&request.username) else {
        unsafe { pam::pam_end(handle, pam::PAM_USER_UNKNOWN) };
        return Err(AuthFailure::InvalidCredentials);
    };

    Ok((PamSession::new(handle, conversation), user))
}
