use std::ffi::CStr;

use limes_proto::AuthFailure;

use crate::auth::pam::raw as pam;

pub(super) fn internal_failure(error: impl std::fmt::Display) -> AuthFailure {
    AuthFailure::Internal(error.to_string())
}

pub(super) fn map_auth_status(status: i32, handle: *mut pam::PamHandle) -> AuthFailure {
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

pub(super) fn pam_error(handle: *mut pam::PamHandle, status: i32) -> String {
    let message = unsafe { pam::pam_strerror(handle, status) };
    if message.is_null() {
        format!("PAM error {status}")
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}
