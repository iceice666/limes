use std::ffi::{CStr, CString, c_void};

use limes_proto::{LimesEvent, PamMessageKind};
use zeroize::Zeroize;

use crate::auth::pam::raw as pam;
use crate::events::EventBus;

pub(super) struct SecretCString {
    bytes: Vec<u8>,
}

impl SecretCString {
    pub(super) fn new(value: &str) -> std::result::Result<Self, std::ffi::NulError> {
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

pub(super) struct PamConversation {
    username: CString,
    username_string: String,
    password: Option<SecretCString>,
    events: Option<EventBus>,
}

impl PamConversation {
    pub(super) fn new(
        username: CString,
        username_string: String,
        password: Option<SecretCString>,
        events: Option<EventBus>,
    ) -> Self {
        Self {
            username,
            username_string,
            password,
            events,
        }
    }

    pub(super) fn username_ptr(&self) -> *const libc::c_char {
        self.username.as_ptr()
    }

    fn password_ptr(&self) -> Option<*const libc::c_char> {
        self.password.as_ref().map(SecretCString::as_ptr)
    }

    pub(super) fn clear_password(&mut self) {
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

pub(super) extern "C" fn pam_conversation(
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

pub(super) fn free_pam_replies(replies: *mut pam::PamResponse, initialized: isize) {
    for index in 0..initialized {
        let response = unsafe { (*replies.offset(index)).resp };
        if !response.is_null() {
            unsafe { libc::free(response.cast()) };
        }
    }
    unsafe { libc::free(replies.cast()) };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    use std::sync::{Arc, Mutex};

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
        PamConversation::new(
            CString::new(username).unwrap(),
            username.to_owned(),
            password.map(|password| SecretCString::new(password).unwrap()),
            events,
        )
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
