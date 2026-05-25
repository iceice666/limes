#[allow(non_camel_case_types)]
pub(super) enum PamHandle {}

#[repr(C)]
pub(super) struct PamMessage {
    pub(super) msg_style: libc::c_int,
    pub(super) msg: *const libc::c_char,
}

#[repr(C)]
pub(super) struct PamResponse {
    pub(super) resp: *mut libc::c_char,
    pub(super) resp_retcode: libc::c_int,
}

#[repr(C)]
pub(super) struct PamConv {
    pub(super) conv: Option<
        extern "C" fn(
            libc::c_int,
            *mut *const PamMessage,
            *mut *mut PamResponse,
            *mut std::ffi::c_void,
        ) -> libc::c_int,
    >,
    pub(super) appdata_ptr: *mut std::ffi::c_void,
}

pub(super) const PAM_SUCCESS: libc::c_int = 0;
pub(super) const PAM_PERM_DENIED: libc::c_int = 6;
pub(super) const PAM_AUTH_ERR: libc::c_int = 7;
pub(super) const PAM_CRED_INSUFFICIENT: libc::c_int = 8;
pub(super) const PAM_AUTHINFO_UNAVAIL: libc::c_int = 9;
pub(super) const PAM_USER_UNKNOWN: libc::c_int = 10;
pub(super) const PAM_MAXTRIES: libc::c_int = 11;
pub(super) const PAM_NEW_AUTHTOK_REQD: libc::c_int = 12;
pub(super) const PAM_ACCT_EXPIRED: libc::c_int = 13;
pub(super) const PAM_CONV_ERR: libc::c_int = 19;
pub(super) const PAM_BUF_ERR: libc::c_int = 5;

pub(super) const PAM_ESTABLISH_CRED: libc::c_int = 0x0002;
pub(super) const PAM_DELETE_CRED: libc::c_int = 0x0004;
pub(super) const PAM_TTY: libc::c_int = 3;

pub(super) const PAM_PROMPT_ECHO_OFF: libc::c_int = 1;
pub(super) const PAM_PROMPT_ECHO_ON: libc::c_int = 2;
pub(super) const PAM_ERROR_MSG: libc::c_int = 3;
pub(super) const PAM_TEXT_INFO: libc::c_int = 4;

#[link(name = "pam")]
unsafe extern "C" {
    pub(super) fn pam_start(
        service_name: *const libc::c_char,
        user: *const libc::c_char,
        pam_conversation: *const PamConv,
        pamh: *mut *mut PamHandle,
    ) -> libc::c_int;
    pub(super) fn pam_end(pamh: *mut PamHandle, pam_status: libc::c_int) -> libc::c_int;
    pub(super) fn pam_authenticate(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
    pub(super) fn pam_acct_mgmt(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
    pub(super) fn pam_setcred(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
    pub(super) fn pam_open_session(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
    pub(super) fn pam_close_session(pamh: *mut PamHandle, flags: libc::c_int) -> libc::c_int;
    pub(super) fn pam_set_item(
        pamh: *mut PamHandle,
        item_type: libc::c_int,
        item: *const std::ffi::c_void,
    ) -> libc::c_int;
    pub(super) fn pam_strerror(pamh: *mut PamHandle, errnum: libc::c_int) -> *const libc::c_char;
    pub(super) fn pam_getenvlist(pamh: *mut PamHandle) -> *mut *mut libc::c_char;
}
