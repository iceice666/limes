use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, AuthSuccess, LimesEvent, PamMessageKind};
use zeroize::Zeroize;

use crate::error::{LimesError, Result};
use crate::events::EventBus;

pub const PAM_SERVICE: &str = "limes";

/// Full authentication backend used by login managers.
///
/// `open_session` opens the PAM/login session before launching the user's
/// desktop command, and `close_session` closes or cleans up the opaque
/// authentication transaction returned in `AuthSuccess::auth_session_id`.
pub trait AuthBackend: Send + Sync {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome;
    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>>;
    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()>;
}

/// Narrow authentication backend used by lock flows.
///
/// Locking only needs to authenticate an unlock request and clean up the opaque
/// auth transaction after success; it must not open a login session.
pub trait LockAuthBackend: Send + Sync {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome;
    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()>;
}

/// Authentication backend placeholder for tests and explicit no-backend setups.
///
/// This backend never authenticates users and cannot open login sessions. It is
/// useful when callers need to construct lock/login runtime pieces without
/// configuring PAM, but it must not be treated as a real security backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopLockBackend;

impl AuthBackend for NoopLockBackend {
    fn authenticate(&self, _request: &AuthRequest) -> AuthOutcome {
        Err(AuthFailure::BackendUnavailable(
            "no lock authentication backend is configured".to_owned(),
        ))
    }

    fn open_session(&self, user: &AuthSuccess) -> Result<Vec<(String, String)>> {
        Err(LimesError::Auth(format!(
            "noop lock backend cannot open a login session for {}",
            user.username
        )))
    }

    fn close_session(&self, _auth_session_id: Option<&str>) -> Result<()> {
        Ok(())
    }
}

impl<T: AuthBackend + ?Sized> LockAuthBackend for T {
    fn authenticate(&self, request: &AuthRequest) -> AuthOutcome {
        AuthBackend::authenticate(self, request)
    }

    fn close_session(&self, auth_session_id: Option<&str>) -> Result<()> {
        AuthBackend::close_session(self, auth_session_id)
    }
}

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

    let mut conversation = Box::new(PamConversation {
        username,
        username_string: request.username.clone(),
        password: Some(password),
        events,
    });
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

struct SecretCString {
    bytes: Vec<u8>,
}

impl SecretCString {
    fn new(value: &str) -> std::result::Result<Self, std::ffi::NulError> {
        CString::new(value).map(|value| Self {
            bytes: value.into_bytes_with_nul(),
        })
    }

    fn as_ptr(&self) -> *const libc::c_char {
        self.bytes.as_ptr().cast()
    }
}

impl Drop for SecretCString {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

struct PamSession {
    handle: *mut pam::PamHandle,
    conversation: Box<PamConversation>,
    opened: bool,
    credentials_established: bool,
    last_status: i32,
}

unsafe impl Send for PamSession {}

impl std::fmt::Debug for PamSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PamSession")
            .field("handle", &self.handle)
            .field("opened", &self.opened)
            .field("credentials_established", &self.credentials_established)
            .field("last_status", &self.last_status)
            .finish_non_exhaustive()
    }
}

impl PamSession {
    fn new(handle: *mut pam::PamHandle, conversation: Box<PamConversation>) -> Self {
        Self {
            handle,
            conversation,
            opened: false,
            credentials_established: false,
            last_status: pam::PAM_SUCCESS,
        }
    }

    fn is_open(&self) -> bool {
        self.opened
    }

    fn establish_credentials(&mut self) -> std::result::Result<(), AuthFailure> {
        let status = unsafe { pam::pam_setcred(self.handle, pam::PAM_ESTABLISH_CRED) };
        self.last_status = status;
        if status != pam::PAM_SUCCESS {
            return Err(map_auth_status(status, self.handle));
        }
        self.credentials_established = true;
        self.conversation.clear_password();
        Ok(())
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
            if self.credentials_established {
                pam::pam_setcred(self.handle, pam::PAM_DELETE_CRED);
            }
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
            pam::PAM_PROMPT_ECHO_ON => Some(state.username_ptr()),
            pam::PAM_PROMPT_ECHO_OFF => {
                let Some(password) = state.password_ptr() else {
                    free_pam_replies(replies, index);
                    return pam::PAM_CONV_ERR;
                };
                Some(password)
            }
            pam::PAM_ERROR_MSG | pam::PAM_TEXT_INFO => None,
            _ => None,
        };

        if let Some(response) = response {
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
    username: CString,
    username_string: String,
    password: Option<SecretCString>,
    events: Option<EventBus>,
}

impl PamConversation {
    fn username_ptr(&self) -> *const libc::c_char {
        self.username.as_ptr()
    }

    fn password_ptr(&self) -> Option<*const libc::c_char> {
        self.password.as_ref().map(SecretCString::as_ptr)
    }

    fn clear_password(&mut self) {
        self.password = None;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::events::EventSink;

    #[derive(Default)]
    struct CapturingSink {
        events: Mutex<Vec<LimesEvent>>,
    }

    impl EventSink for CapturingSink {
        fn emit(&self, event: &LimesEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    #[test]
    fn noop_lock_backend_rejects_authentication() {
        let backend = NoopLockBackend;
        let request = AuthRequest::new("alice", "secret");

        let outcome = AuthBackend::authenticate(&backend, &request);

        assert_eq!(
            outcome,
            Err(AuthFailure::BackendUnavailable(
                "no lock authentication backend is configured".to_owned()
            ))
        );
    }

    #[test]
    fn noop_lock_backend_is_usable_as_lock_auth_backend() {
        let backend: &dyn LockAuthBackend = &NoopLockBackend;
        let request = AuthRequest::new("alice", "secret");

        assert!(matches!(
            backend.authenticate(&request),
            Err(AuthFailure::BackendUnavailable(_))
        ));
        assert!(backend.close_session(Some("unused")).is_ok());
    }

    #[test]
    fn noop_lock_backend_cannot_open_login_sessions() {
        let backend = NoopLockBackend;
        let success = AuthSuccess {
            username: "alice".to_owned(),
            uid: 1000,
            gid: 1000,
            home: Some("/home/alice".to_owned()),
            shell: Some("/bin/sh".to_owned()),
            auth_session_id: None,
        };

        let error = backend.open_session(&success).unwrap_err();

        assert!(matches!(error, LimesError::Auth(_)));
    }

    #[test]
    fn pam_conversation_answers_password_prompt_while_secret_is_present() {
        let mut conversation = test_conversation("alice", Some("secret"), None);
        let message_text = CString::new("Password:").unwrap();
        let message = pam::PamMessage {
            msg_style: pam::PAM_PROMPT_ECHO_OFF,
            msg: message_text.as_ptr(),
        };

        let (status, responses) = call_conversation(&mut conversation, &[&message]);

        assert_eq!(status, pam::PAM_SUCCESS);
        let responses = responses.expect("PAM conversation should allocate responses");
        let response = unsafe { CStr::from_ptr((*responses).resp) };
        assert_eq!(response.to_str().unwrap(), "secret");
        free_pam_replies(responses, 1);
    }

    #[test]
    fn pam_conversation_rejects_password_prompt_after_secret_is_cleared() {
        let mut conversation = test_conversation("alice", Some("secret"), None);
        conversation.clear_password();
        let message_text = CString::new("Password:").unwrap();
        let message = pam::PamMessage {
            msg_style: pam::PAM_PROMPT_ECHO_OFF,
            msg: message_text.as_ptr(),
        };

        let (status, responses) = call_conversation(&mut conversation, &[&message]);

        assert_eq!(status, pam::PAM_CONV_ERR);
        assert!(responses.is_none());
    }

    #[test]
    fn pam_conversation_emits_info_messages_without_password() {
        let events = EventBus::new();
        let sink = Arc::new(CapturingSink::default());
        events.subscribe(sink.clone());
        let mut conversation = test_conversation("alice", None, Some(events));
        let message_text = CString::new("hello").unwrap();
        let message = pam::PamMessage {
            msg_style: pam::PAM_TEXT_INFO,
            msg: message_text.as_ptr(),
        };

        let (status, responses) = call_conversation(&mut conversation, &[&message]);

        assert_eq!(status, pam::PAM_SUCCESS);
        let responses = responses.expect("PAM conversation should allocate responses");
        assert!(unsafe { (*responses).resp }.is_null());
        free_pam_replies(responses, 1);

        let captured = sink.events.lock().unwrap();
        assert_eq!(
            *captured,
            vec![LimesEvent::AuthPamMessage {
                username: "alice".to_owned(),
                kind: PamMessageKind::TextInfo,
                message: "hello".to_owned(),
            }]
        );
    }

    fn test_conversation(
        username: &str,
        password: Option<&str>,
        events: Option<EventBus>,
    ) -> PamConversation {
        PamConversation {
            username: CString::new(username).unwrap(),
            username_string: username.to_owned(),
            password: password.map(|password| SecretCString::new(password).unwrap()),
            events,
        }
    }

    fn call_conversation(
        conversation: &mut PamConversation,
        messages: &[&pam::PamMessage],
    ) -> (libc::c_int, Option<*mut pam::PamResponse>) {
        let mut raw_messages = messages
            .iter()
            .map(|message| *message as *const pam::PamMessage)
            .collect::<Vec<_>>();
        let mut responses = ptr::null_mut();
        let status = pam_conversation(
            raw_messages.len() as libc::c_int,
            raw_messages.as_mut_ptr(),
            &mut responses,
            (conversation as *mut PamConversation).cast(),
        );

        (
            status,
            if responses.is_null() {
                None
            } else {
                Some(responses)
            },
        )
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
