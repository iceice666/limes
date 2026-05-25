use std::ffi::CStr;
use std::ptr;

use limes_proto::AuthFailure;

use crate::auth::pam::conversation::PamConversation;
use crate::auth::pam::raw as pam;
use crate::auth::pam::status::{map_auth_status, pam_error};
use crate::{LimesError, Result};

pub(super) struct PamSession {
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
    pub(super) fn new(handle: *mut pam::PamHandle, conversation: Box<PamConversation>) -> Self {
        Self {
            handle,
            conversation,
            opened: false,
            credentials_established: false,
            last_status: pam::PAM_SUCCESS,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.opened
    }

    pub(super) fn establish_credentials(&mut self) -> std::result::Result<(), AuthFailure> {
        let status = unsafe { pam::pam_setcred(self.handle, pam::PAM_ESTABLISH_CRED) };
        self.last_status = status;
        if status != pam::PAM_SUCCESS {
            return Err(map_auth_status(status, self.handle));
        }
        self.credentials_established = true;
        self.conversation.clear_password();
        Ok(())
    }

    pub(super) fn open(&mut self) -> Result<()> {
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

    pub(super) fn env(&self) -> Vec<(String, String)> {
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
