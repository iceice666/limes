use std::ffi::{CStr, CString};
use std::ptr;

#[derive(Debug, Clone)]
pub(super) struct UserRecord {
    pub(super) username: String,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) home: Option<String>,
    pub(super) shell: Option<String>,
}

pub(super) fn lookup_user(username: &str) -> Option<UserRecord> {
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
