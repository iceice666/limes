use std::sync::Arc;

use iced::widget::{button, column, container, row, text, text_input};
use iced::{Alignment, Element, Fill, Task, Theme};
use limes_core::{DisplayBackend, EventBus, LockManager, PamAuth, Result, StderrEventSink};
use limes_proto::{AuthFailure, AuthOutcome, AuthRequest, LockState};

fn main() -> iced::Result {
    iced::application("limes simple lock", SimpleLock::update, SimpleLock::view)
        .theme(|_| Theme::Dark)
        .run_with(SimpleLock::new)
}

struct SimpleLock {
    manager: Arc<LockManager>,
    username: String,
    password: String,
    state: LockState,
    status: String,
    authenticating: bool,
}

#[derive(Debug, Clone)]
enum Message {
    UsernameChanged(String),
    PasswordChanged(String),
    Lock,
    Submit,
    AuthFinished(AuthOutcome),
}

impl SimpleLock {
    fn new() -> (Self, Task<Message>) {
        let events = EventBus::new();
        events.subscribe(Arc::new(StderrEventSink));

        let manager = Arc::new(LockManager::new(
            Arc::new(DemoDisplayBackend),
            Arc::new(PamAuth::with_events(Some(events.clone()))),
            events,
        ));

        let initial_state = match manager.lock_now() {
            Ok(()) => LockState::Locked,
            Err(error) => {
                eprintln!("failed to enter demo lock state: {error}");
                LockState::Unlocked
            }
        };

        (
            Self {
                manager,
                username: std::env::var("USER").unwrap_or_default(),
                password: String::new(),
                state: initial_state,
                status: "Demo lock active. Enter your PAM password to unlock.".to_owned(),
                authenticating: false,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UsernameChanged(username) => {
                self.username = username;
                Task::none()
            }
            Message::PasswordChanged(password) => {
                self.password = password;
                Task::none()
            }
            Message::Lock => {
                self.password.clear();
                match self.manager.lock_now() {
                    Ok(()) => {
                        self.state = LockState::Locked;
                        self.status = "Locked. Enter your password to unlock.".to_owned();
                    }
                    Err(error) => {
                        self.state = self.manager.state().unwrap_or(LockState::Unlocked);
                        self.status = format!("Could not lock: {error}");
                    }
                }
                Task::none()
            }
            Message::Submit => {
                if self.authenticating || self.state != LockState::Locked {
                    return Task::none();
                }

                let manager = Arc::clone(&self.manager);
                let username = self.username.clone();
                let password = std::mem::take(&mut self.password);
                self.authenticating = true;
                self.state = LockState::Unlocking;
                self.status = "Authenticating with PAM...".to_owned();

                Task::perform(
                    async move {
                        let mut request = AuthRequest::new(username, password);
                        let outcome = manager.unlock(&request);
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
        let warning =
            text("Example only: this window does not provide a secure compositor/session lock.")
                .size(16);
        let state = text(format!("State: {}", self.state)).size(20);
        let status = text(&self.status).size(18);

        let username = text_input("username", &self.username)
            .on_input(Message::UsernameChanged)
            .padding(12)
            .size(20);

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

        let controls = row![unlock, lock_again].spacing(12);

        let content = column![title, warning, state, username, password, controls, status]
            .spacing(16)
            .padding(32)
            .align_x(Alignment::Center)
            .max_width(520);

        container(content)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .center_y(Fill)
            .into()
    }
}

fn auth_error_message(error: &AuthFailure) -> String {
    match error {
        AuthFailure::InvalidCredentials => "Invalid username or password.".to_owned(),
        AuthFailure::LockedOut => "Account is locked out.".to_owned(),
        AuthFailure::BackendUnavailable(reason) => format!("PAM backend unavailable: {reason}"),
        AuthFailure::Internal(reason) => format!("Authentication error: {reason}"),
    }
}

#[derive(Debug, Default)]
struct DemoDisplayBackend;

impl DisplayBackend for DemoDisplayBackend {
    fn lock(&self) -> Result<()> {
        Ok(())
    }

    fn unlock(&self) -> Result<()> {
        Ok(())
    }
}
