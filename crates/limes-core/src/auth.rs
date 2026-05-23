use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, PamMessageKind};

use crate::error::{LimesError, Result};
use crate::events::EventBus;

pub const PAM_SERVICE: &str = "limes";

pub trait AuthBackend: Send + Sync {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome;
    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>>;
    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()>;
}

pub struct PamAuth {
    service: &'static str,
    sessions: Mutex<HashMap<String, PamSession>>,
    next_session_id: AtomicU64,
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

impl AuthBackend for PamAuth {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        self.cleanup_pending_auth_state()
            .map_err(internal_failure)?;

        let service = CString::new(self.service).map_err(internal_failure)?;
        let username = CString::new(request.username.as_str()).map_err(internal_failure)?;
        let password = CString::new(request.password.as_str()).map_err(internal_failure)?;
        let tty = request
            .tty
            .as_deref()
            .filter(|tty| !tty.trim().is_empty())
            .map(CString::new)
            .transpose()
            .map_err(internal_failure)?;

        let mut conversation = PamConversation {
            username: username.as_ptr(),
            username_string: request.username.clone(),
            password: password.as_ptr(),
            events: self.events.clone(),
        };
        let conv = pam::PamConv {
            conv: Some(pam_conversation),
            appdata_ptr: (&mut conversation as *mut PamConversation).cast(),
        };

        let mut handle = ptr::null_mut();
        let mut status =
            unsafe { pam::pam_start(service.as_ptr(), username.as_ptr(), &conv, &mut handle) };
        if status != pam::PAM_SUCCESS {
            return Err(map_auth_status(status, handle));
        }

        if let Some(tty) = &tty {
            status = unsafe { pam::pam_set_item(handle, pam::PAM_TTY, tty.as_ptr().cast()) };
            if status != pam::PAM_SUCCESS {
                unsafe { pam::pam_end(handle, status) };
                return Err(map_auth_status(status, ptr::null_mut()));
            }
        }

        status = unsafe { pam::pam_authenticate(handle, 0) };
        if status != pam::PAM_SUCCESS {
            unsafe { pam::pam_end(handle, status) };
            return Err(map_auth_status(status, ptr::null_mut()));
        }

        status = unsafe { pam::pam_acct_mgmt(handle, 0) };
        if status != pam::PAM_SUCCESS {
            unsafe { pam::pam_end(handle, status) };
            return Err(map_auth_status(status, ptr::null_mut()));
        }

        status = unsafe { pam::pam_setcred(handle, pam::PAM_ESTABLISH_CRED) };
        if status != pam::PAM_SUCCESS {
            unsafe { pam::pam_end(handle, status) };
            return Err(map_auth_status(status, ptr::null_mut()));
        }

        let new_session = PamSession::new(handle);
        let user = match lookup_user(&request.username) {
            Some(user) => user,
            None => {
                drop(new_session);
                return Err(AuthFailure::InvalidCredentials);
            }
        };
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

#[derive(Debug)]
struct PamSession {
    handle: *mut pam::PamHandle,
    opened: bool,
    last_status: i32,
}

unsafe impl Send for PamSession {}

impl PamSession {
    fn new(handle: *mut pam::PamHandle) -> Self {
        Self {
            handle,
            opened: false,
            last_status: pam::PAM_SUCCESS,
        }
    }

    fn is_open(&self) -> bool {
        self.opened
    }

    fn open(&mut self) -> Result<()> {
        if self.opened {
            return Ok(());
        }

        let status = unsafe { pam::pam_open_session(self.handle, 0) };
        self.last_status = status;
        if status != pam::PAM_SUCCESS {
            return Err(LimesError::Auth(pam_error(self.handle, status)));
        }
        self.opened = true;
        Ok(())
    }

    fn env(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let envp = unsafe { pam::pam_getenvlist(self.handle) };
        if envp.is_null() {
            return out;
        }

        let mut cursor = envp;
        loop {
            let item = unsafe { *cursor };
            if item.is_null() {
                break;
            }
            let value = unsafe { CStr::from_ptr(item) }.to_string_lossy();
            if let Some((key, value)) = value.split_once('=') {
                out.push((key.to_owned(), value.to_owned()));
            }
            unsafe { libc::free(item.cast()) };
            cursor = unsafe { cursor.add(1) };
        }
        unsafe { libc::free(envp.cast()) };
        out
    }
}

impl Drop for PamSession {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        if self.opened {
            self.last_status = unsafe { pam::pam_close_session(self.handle, 0) };
        }
        unsafe {
            pam::pam_setcred(self.handle, pam::PAM_DELETE_CRED);
            pam::pam_end(self.handle, self.last_status);
        }
        self.handle = ptr::null_mut();
    }
}

#[derive(Debug, Clone)]
struct UserRecord {
    username: String,
    uid: u32,
    gid: u32,
    home: Option<String>,
    shell: Option<String>,
}

fn lookup_user(username: &str) -> Option<UserRecord> {
    let username = CString::new(username).ok()?;
    let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = ptr::null_mut();
    let mut buffer_len = match unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) } {
        n if n > 0 => n as usize,
        _ => 16 * 1024,
    };

    loop {
        let mut buffer = vec![0_u8; buffer_len];
        let rc = unsafe {
            libc::getpwnam_r(
                username.as_ptr(),
                &mut passwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if rc == libc::ERANGE {
            buffer_len = buffer_len.saturating_mul(2);
            if buffer_len > 1024 * 1024 {
                return None;
            }
            continue;
        }

        if rc != 0 || result.is_null() {
            return None;
        }

        return Some(UserRecord {
            username: c_string_field(passwd.pw_name)?,
            uid: passwd.pw_uid as u32,
            gid: passwd.pw_gid as u32,
            home: optional_c_string_field(passwd.pw_dir),
            shell: optional_c_string_field(passwd.pw_shell),
        });
    }
}

fn optional_c_string_field(value: *const libc::c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn c_string_field(value: *const libc::c_char) -> Option<String> {
    optional_c_string_field(value).filter(|value| !value.is_empty())
}

extern "C" fn pam_conversation(
    num_msg: libc::c_int,
    msg: *mut *const pam::PamMessage,
    resp: *mut *mut pam::PamResponse,
    appdata_ptr: *mut c_void,
) -> libc::c_int {
    if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata_ptr.is_null() {
        return pam::PAM_CONV_ERR;
    }

    let replies = unsafe { libc::calloc(num_msg as usize, std::mem::size_of::<pam::PamResponse>()) }
        as *mut pam::PamResponse;
    if replies.is_null() {
        return pam::PAM_BUF_ERR;
    }

    let state = unsafe { &*(appdata_ptr.cast::<PamConversation>()) };
    for index in 0..num_msg as isize {
        let message = unsafe { *msg.offset(index) };
        if message.is_null() {
            free_pam_replies(replies, index);
            return pam::PAM_CONV_ERR;
        }

        let style = unsafe { (*message).msg_style };
        let message_text = pam_message_text(message);
        if let Some(kind) = pam_message_kind(style) {
            state.emit_message(kind, &message_text);
        }

        let response = match style {
            pam::PAM_PROMPT_ECHO_ON => state.username,
            pam::PAM_PROMPT_ECHO_OFF => state.password,
            pam::PAM_ERROR_MSG | pam::PAM_TEXT_INFO => ptr::null(),
            _ => ptr::null(),
        };

        if !response.is_null() {
            let duplicated = unsafe { libc::strdup(response) };
            if duplicated.is_null() {
                free_pam_replies(replies, index);
                return pam::PAM_BUF_ERR;
            }
            unsafe { (*replies.offset(index)).resp = duplicated };
        }
    }

    unsafe { *resp = replies };
    pam::PAM_SUCCESS
}

fn free_pam_replies(replies: *mut pam::PamResponse, initialized: isize) {
    for index in 0..initialized {
        let response = unsafe { (*replies.offset(index)).resp };
        if !response.is_null() {
            unsafe { libc::free(response.cast()) };
        }
    }
    unsafe { libc::free(replies.cast()) };
}

struct PamConversation {
    username: *const libc::c_char,
    username_string: String,
    password: *const libc::c_char,
    events: Option<EventBus>,
}

impl PamConversation {
    fn emit_message(&self, kind: PamMessageKind, message: &str) {
        if let Some(events) = &self.events {
            events.emit(LimesEvent::AuthPamMessage {
                username: self.username_string.clone(),
                kind,
                message: message.to_owned(),
            });
        }
    }
}

fn pam_message_kind(style: libc::c_int) -> Option<PamMessageKind> {
    match style {
        pam::PAM_PROMPT_ECHO_ON => Some(PamMessageKind::PromptEchoOn),
        pam::PAM_PROMPT_ECHO_OFF => Some(PamMessageKind::PromptEchoOff),
        pam::PAM_TEXT_INFO => Some(PamMessageKind::TextInfo),
        pam::PAM_ERROR_MSG => Some(PamMessageKind::Error),
        _ => None,
    }
}

fn pam_message_text(message: *const pam::PamMessage) -> String {
    let text = unsafe { (*message).msg };
    if text.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }
}

fn internal_failure(error: impl std::fmt::Display) -> AuthFailure {
    AuthFailure::Internal(error.to_string())
}

fn map_auth_status(status: i32, handle: *mut pam::PamHandle) -> AuthFailure {
    match status {
        pam::PAM_AUTH_ERR | pam::PAM_USER_UNKNOWN => AuthFailure::InvalidCredentials,
        pam::PAM_MAXTRIES => AuthFailure::LockedOut,
        pam::PAM_CRED_INSUFFICIENT | pam::PAM_AUTHINFO_UNAVAIL => {
            AuthFailure::BackendUnavailable(pam_error(handle, status))
        }
        pam::PAM_ACCT_EXPIRED | pam::PAM_NEW_AUTHTOK_REQD | pam::PAM_PERM_DENIED => {
            AuthFailure::Internal(pam_error(handle, status))
        }
        _ => AuthFailure::Internal(pam_error(handle, status)),
    }
}

fn pam_error(handle: *mut pam::PamHandle, status: i32) -> String {
    let message = unsafe { pam::pam_strerror(handle, status) };
    if message.is_null() {
        format!("PAM error {status}")
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}

#[allow(non_camel_case_types)]
mod pam {
    use std::ffi::c_void;

    pub enum PamHandle {}

    #[repr(C)]
    pub struct PamMessage {
        pub msg_style: libc::c_int,
        pub msg: *const libc::c_char,
    }

    #[repr(C)]
    pub struct PamResponse {
        pub resp: *mut libc::c_char,
        pub resp_retcode: libc::c_int,
    }

    #[repr(C)]
    pub struct PamConv {
        pub conv: Option<
            extern "C" fn(
                libc::c_int,
                *mut *const PamMessage,
                *mut *mut PamResponse,
                *mut c_void,
            ) -> libc::c_int,
        >,
        pub appdata_ptr: *mut c_void,
    }

    pub const PAM_SUCCESS: libc::c_int = 0;
    pub const PAM_PERM_DENIED: libc::c_int = 6;
    pub const PAM_AUTH_ERR: libc::c_int = 7;
    pub const PAM_CRED_INSUFFICIENT: libc::c_int = 8;
    pub const PAM_AUTHINFO_UNAVAIL: libc::c_int = 9;
    pub const PAM_USER_UNKNOWN: libc::c_int = 10;
    pub const PAM_MAXTRIES: libc::c_int = 11;
    pub const PAM_NEW_AUTHTOK_REQD: libc::c_int = 12;
    pub const PAM_ACCT_EXPIRED: libc::c_int = 13;
    pub const PAM_CONV_ERR: libc::c_int = 19;
    pub const PAM_BUF_ERR: libc::c_int = 5;

    pub const PAM_ESTABLISH_CRED: libc::c_int = 0x0002;
    pub const PAM_DELETE_CRED: libc::c_int = 0x0004;
    pub const PAM_TTY: libc::c_int = 3;

    pub const PAM_PROMPT_ECHO_OFF: libc::c_int = 1;
    pub const PAM_PROMPT_ECHO_ON: libc::c_int = 2;
    pub const PAM_ERROR_MSG: libc::c_int = 3;
    pub const PAM_TEXT_INFO: libc::c_int = 4;

    #[link(name = "pam")]
    unsafe extern "C" {
        pub fn pam_start(
            service_name: *const libc::c_char,
            user: *const libc::c_char,
            pam_conversation: *const PamConv,
            pamh: *mut *mut PamHandle,
        ) -> libc::c_int;
        pub fn pam_end(pamh: *mut PamHandle, pam_status: libc::c_int) -> libc::c_int;
        pub fn pam_authenticate(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
        pub fn pam_acct_mgmt(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
        pub fn pam_setcred(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
        pub fn pam_open_session(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
        pub fn pam_close_session(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
        pub fn pam_set_item(
            pamh: *mut PamHandle,
            item_type: libc::c_int,
            item: *const c_void,
        ) -> libc::c_int;
        pub fn pam_strerror(pamh: *mut PamHandle, errnum: libc::c_int) -> *const libc::c_char;
        pub fn pam_getenvlist(pamh: *mut PamHandle) -> *mut *mut libc::c_char;
    }
}
