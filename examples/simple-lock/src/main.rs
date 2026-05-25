use std::{process, sync::Arc};

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, column, container, row, text, text_input},
    window,
};
use iced_sessionlock::{actions::UnLockAction, application};
use limes_lock::{
    AuthFailure as ProtoAuthFailure, AuthOutcome, AuthRequest, LockRuntime, LockState,
    StderrEventSink,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("limes-simple-lock: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let runtime = Arc::new(
        LockRuntime::from_env()
            .map_err(|error| format!("cannot initialize limes lock runtime: {error}"))?,
    );
    runtime.events().subscribe(Arc::new(StderrEventSink));

    application(
        move || SimpleLock::new(Arc::clone(&runtime)),
        SimpleLock::update,
        SimpleLock::view,
    )
    .run()
    .map_err(|error| error.to_string())
}

struct SimpleLock {
    runtime: Arc<LockRuntime>,
    username: String,
    password: String,
    state: LockState,
    status: String,
    authenticating: bool,
}

#[derive(Debug, Clone)]
enum Message {
    PasswordChanged(String),
    Submit,
    AuthFinished(AuthOutcome),
    UnlockSession,
}

impl TryFrom<Message> for UnLockAction {
    type Error = Message;

    fn try_from(value: Message) -> std::result::Result<Self, Self::Error> {
        match value {
            Message::UnlockSession => Ok(UnLockAction),
            other => Err(other),
        }
    }
}

impl SimpleLock {
    fn new(runtime: Arc<LockRuntime>) -> (Self, Task<Message>) {
        (
            Self {
                runtime,
                username: std::env::var("USER").unwrap_or_default(),
                password: String::new(),
                state: LockState::Locked,
                status: "Enter your PAM password and press Enter to unlock.".to_owned(),
                authenticating: false,
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
            Message::Submit => {
                if self.authenticating || self.state != LockState::Locked {
                    return Task::none();
                }

                let runtime = Arc::clone(&self.runtime);
                let username = self.username.clone();
                let password = std::mem::take(&mut self.password);
                self.authenticating = true;
                self.state = LockState::Unlocking;
                self.status = "Authenticating with PAM...".to_owned();

                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut request = AuthRequest::new(username, password);
                            let outcome = runtime.authenticate_unlock(&request);
                            request.clear_secret();
                            outcome
                        })
                        .await
                        .unwrap_or_else(|error| {
                            Err(ProtoAuthFailure::Internal(format!(
                                "authentication task failed: {error}"
                            )))
                        })
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
                        return Task::done(Message::UnlockSession);
                    }
                    Err(error) => {
                        self.state = LockState::Locked;
                        self.status = auth_error_message(error);
                    }
                }
                Task::none()
            }
            Message::UnlockSession => Task::none(),
        }
    }

    fn view(&self, _window: window::Id) -> Element<'_, Message> {
        let title = text("limes simple lock").size(36);
        let warning = text(
            "Uses Wayland ext-session-lock-v1 through iced_sessionlock. limes authenticates the unlock request.",
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

        let controls = row![unlock].spacing(12);

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
