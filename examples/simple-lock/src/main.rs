use std::sync::Arc;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, column, container, row, text, text_input},
};
use iced_layershell::{
    actions::LayerShellCustomActionWithId,
    application,
    reexport::Anchor,
    settings::{LayerShellSettings, Settings},
};
use limes_core::{Runtime, StderrEventSink};

use limes_proto::{AuthFailure as ProtoAuthFailure, AuthOutcome, AuthRequest, LockState};

fn main() -> iced_layershell::Result {
    application(
        SimpleLock::new,
        || "limes simple lock".to_owned(),
        SimpleLock::update,
        SimpleLock::view,
    )
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}

struct SimpleLock {
    runtime: Option<Arc<Runtime>>,
    username: String,
    password: String,
    state: LockState,
    status: String,
    authenticating: bool,
    lock_frontend: bool,
}

#[derive(Debug, Clone)]
enum Message {
    PasswordChanged(String),
    Lock,
    Submit,
    AuthFinished(AuthOutcome),
}

impl TryFrom<Message> for LayerShellCustomActionWithId {
    type Error = Message;

    fn try_from(value: Message) -> std::result::Result<Self, Self::Error> {
        Err(value)
    }
}

impl SimpleLock {
    fn new() -> (Self, Task<Message>) {
        let lock_frontend = matches!(std::env::args().nth(1).as_deref(), Some("lock"));
        let runtime = Runtime::from_env().ok().map(Arc::new);

        let (initial_state, status) = if let Some(runtime) = &runtime {
            runtime.events().subscribe(Arc::new(StderrEventSink));
            if lock_frontend {
                match runtime.lock_now() {
                    Ok(()) => (
                        LockState::Locked,
                        "Locked. Enter your PAM password and press Enter to unlock.".to_owned(),
                    ),
                    Err(error) => (LockState::Unlocked, format!("Could not lock: {error}")),
                }
            } else {
                (
                    LockState::Unlocked,
                    "Runtime ready. Press \"Lock again\" to begin a lock test.".to_owned(),
                )
            }
        } else {
            (
                LockState::Unlocked,
                "Runtime unavailable. Cannot initialize lock backend.".to_owned(),
            )
        };

        (
            Self {
                runtime,
                username: std::env::var("USER").unwrap_or_default(),
                password: String::new(),
                state: initial_state,
                status,
                authenticating: false,
                lock_frontend,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PasswordChanged(password) => {
                self.password = password;
                Task::none()
            }
            Message::Lock => {
                if self.runtime.is_none() {
                    self.status = "Runtime unavailable; lock not possible.".to_owned();
                    return Task::none();
                }

                self.password.clear();
                let runtime = Arc::clone(self.runtime.as_ref().expect("runtime present"));
                match runtime.lock_now() {
                    Ok(()) => {
                        self.state = LockState::Locked;
                        self.status =
                            "Locked. Enter your password and press Enter to unlock.".to_owned();
                    }
                    Err(error) => {
                        self.state = LockState::Unlocked;
                        self.status = format!("Could not lock: {error}");
                    }
                }
                Task::none()
            }
            Message::Submit => {
                if self.runtime.is_none() || self.authenticating || self.state != LockState::Locked
                {
                    return Task::none();
                }

                let runtime = Arc::clone(self.runtime.as_ref().expect("runtime present"));
                let username = self.username.clone();
                let password = std::mem::take(&mut self.password);
                self.authenticating = true;
                self.state = LockState::Unlocking;
                self.status = "Authenticating with PAM...".to_owned();

                Task::perform(
                    async move {
                        let mut request = AuthRequest::new(username, password);
                        let outcome = runtime.unlock(&request);
                        request.clear_secret();
                        outcome
                    },
                    Message::AuthFinished,
                )
            }
            Message::AuthFinished(outcome) => {
                self.authenticating = false;
                match &outcome {
                    Ok(success) => {
                        self.state = LockState::Unlocked;
                        self.status =
                            format!("Unlocked as {} (uid {}).", success.username, success.uid);
                    }
                    Err(error) => {
                        self.state = LockState::Locked;
                        self.status = auth_error_message(error);
                    }
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let title = text("limes simple lock").size(36);
        let warning = text(
            "Uses Wayland ext-session-lock-v1 via limes-core. The compositor lock surface is currently delegated to the frontend.",
        )
        .size(16);
        let state = text(format!("State: {}", self.state)).size(20);
        let status = text(&self.status).size(18);

        let username = text(format!("User: {}", self.username)).size(20);

        let password = text_input("password", &self.password)
            .on_input(Message::PasswordChanged)
            .on_submit(Message::Submit)
            .secure(true)
            .padding(12)
            .size(20);

        let unlock = if self.authenticating || self.state != LockState::Locked {
            button("Unlock")
        } else {
            button("Unlock").on_press(Message::Submit)
        };

        let lock_again = if self.state == LockState::Unlocked {
            button("Lock again").on_press(Message::Lock)
        } else {
            button("Lock again")
        };

        let controls = if self.lock_frontend {
            row![unlock]
        } else {
            row![unlock, lock_again]
        }
        .spacing(12);

        let content = column![title, warning, state, username, password, controls, status]
            .spacing(16)
            .padding(32)
            .align_x(Alignment::Center)
            .max_width(520);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

fn auth_error_message(error: &ProtoAuthFailure) -> String {
    match error {
        ProtoAuthFailure::InvalidCredentials => "Invalid username or password.".to_owned(),
        ProtoAuthFailure::LockedOut => "Account is locked out.".to_owned(),
        ProtoAuthFailure::BackendUnavailable(reason) => {
            format!("PAM backend unavailable: {reason}")
        }
        ProtoAuthFailure::Internal(reason) => format!("Authentication error: {reason}"),
    }
}
