use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::{self, Command};
use std::sync::{Arc, Mutex, mpsc};

use iced::alignment::{Horizontal, Vertical};
use iced::futures::SinkExt;
use iced::keyboard::{Key, key};
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Color, Element, Length, Size, Subscription, Task};
use limes_core::{EventSink, LimesError, Result, Runtime};
use limes_proto::{AuthRequest, LimesEvent, PamMessageKind};

fn main() {
    if let Err(error) = run() {
        eprintln!("limes-frontend-native: {error}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("login") | None => login(),
        Some("lock") => lock(),
        Some("--help" | "-h" | "help") => {
            print_help();
            Ok(())
        }
        Some(other) => Err(LimesError::Config(format!(
            "unknown frontend mode `{other}`; expected login or lock"
        ))),
    }
}

fn login() -> Result<()> {
    let runtime = Runtime::from_env()?;
    eprintln!("limes native starter frontend (text renderer)");

    for attempt in 1..=runtime.config().max_auth_attempts {
        let username = prompt_line("username: ")?;
        let password = prompt_secret("password: ")?;
        let mut request = AuthRequest {
            username,
            password,
            tty: env::var("TTY").ok(),
        };

        match runtime.authenticate(&request) {
            Ok(success) => {
                request.clear_secret();
                let handle = runtime.start_session_for(&success)?;
                println!("started session pid {} for {}", handle.pid, handle.username);
                let status = runtime.wait_session(&handle)?;
                println!(
                    "session for {} exited with wait status {}",
                    handle.username, status
                );
                return Ok(());
            }
            Err(reason) => {
                request.clear_secret();
                eprintln!(
                    "authentication failed ({attempt}/{}): {reason}",
                    runtime.config().max_auth_attempts
                );
            }
        }
    }

    Err(LimesError::Auth(
        "maximum authentication attempts reached".to_owned(),
    ))
}

fn lock() -> Result<()> {
    let runtime = Arc::new(Runtime::from_env()?);
    let (pam_tx, pam_rx) = mpsc::channel();
    runtime
        .events()
        .subscribe(Arc::new(GuiPamEventSink { sender: pam_tx }));
    let pam_messages = Arc::new(Mutex::new(pam_rx));
    let username = env::var("LIMES_USERNAME")
        .or_else(|_| env::var("USER"))
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "user".to_owned());
    let flags = LockFlags {
        runtime,
        username,
        pam_messages: PamReceiver(pam_messages),
    };

    iced::application(
        move || LockApp::new(flags.clone()),
        LockApp::update,
        LockApp::view,
    )
    .title("Limes Lock")
    .subscription(LockApp::subscription)
    .window_size(Size::new(520.0, 360.0))
    .resizable(false)
    .run()
    .map_err(|error| LimesError::Frontend(format!("failed to run iced lock UI: {error}")))
}

#[derive(Clone)]
struct LockFlags {
    runtime: Arc<Runtime>,
    username: String,
    pam_messages: PamReceiver,
}

#[derive(Clone)]
struct PamReceiver(Arc<Mutex<mpsc::Receiver<String>>>);

impl std::hash::Hash for PamReceiver {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "limes-pam-messages".hash(state);
    }
}

struct GuiPamEventSink {
    sender: mpsc::Sender<String>,
}

impl EventSink for GuiPamEventSink {
    fn emit(&self, event: &LimesEvent) {
        if let LimesEvent::AuthPamMessage { kind, message, .. } = event {
            let label = match kind {
                PamMessageKind::PromptEchoOn => "PAM prompt",
                PamMessageKind::PromptEchoOff => "PAM secret prompt",
                PamMessageKind::TextInfo => "PAM info",
                PamMessageKind::Error => "PAM error",
            };
            let text = if message.is_empty() {
                label.to_owned()
            } else {
                format!("{label}: {message}")
            };
            let _ = self.sender.send(text);
        }
    }
}

struct LockApp {
    runtime: Arc<Runtime>,
    username: String,
    password: String,
    status: String,
    pam_messages: PamReceiver,
    verifying: bool,
    unlocked: bool,
}

#[derive(Debug, Clone)]
enum LockMessage {
    PasswordChanged(String),
    VerifyRequested,
    AuthFinished(std::result::Result<String, String>),
    BackspacePressed,
    PamMessage(String),
}

impl LockApp {
    fn new(flags: LockFlags) -> Self {
        Self {
            runtime: flags.runtime,
            username: flags.username,
            password: String::new(),
            status: "Enter password, or press Enter with an empty field for PAM/fingerprint"
                .to_owned(),
            pam_messages: flags.pam_messages,
            verifying: false,
            unlocked: false,
        }
    }

    fn subscription(&self) -> Subscription<LockMessage> {
        Subscription::batch([
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed { key, .. } => match key.as_ref() {
                    Key::Named(key::Named::Backspace) => Some(LockMessage::BackspacePressed),
                    _ => None,
                },
                _ => None,
            }),
            Subscription::run_with(self.pam_messages.clone(), pam_message_stream),
        ])
    }

    fn update(&mut self, message: LockMessage) -> Task<LockMessage> {
        match message {
            LockMessage::PasswordChanged(password) => {
                self.password = password;
                Task::none()
            }
            LockMessage::BackspacePressed => {
                self.password.clear();
                self.status = "Input cleared".to_owned();
                Task::none()
            }
            LockMessage::VerifyRequested => {
                if self.verifying {
                    return Task::none();
                }

                self.verifying = true;
                self.status = "Verifying with PAM... check for fingerprint prompts".to_owned();
                let runtime = Arc::clone(&self.runtime);
                let username = self.username.clone();
                let password = std::mem::take(&mut self.password);
                let tty = env::var("TTY").ok();

                Task::perform(
                    async move { verify_request(runtime, username, password, tty) },
                    LockMessage::AuthFinished,
                )
            }
            LockMessage::AuthFinished(result) => {
                self.verifying = false;
                match result {
                    Ok(username) => {
                        self.unlocked = true;
                        self.status = format!("Unlocked as {username}");
                    }
                    Err(reason) => {
                        self.status = format!("Authentication failed: {reason}");
                    }
                }
                Task::none()
            }
            LockMessage::PamMessage(message) => {
                if self.verifying {
                    self.status = message;
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, LockMessage> {
        let title = text("LIMES").size(44);
        let state = if self.unlocked { "UNLOCKED" } else { "LOCKED" };
        let status_color = if self.unlocked {
            Color::from_rgb(0.25, 0.75, 0.35)
        } else if self.verifying {
            Color::from_rgb(0.95, 0.75, 0.25)
        } else {
            Color::from_rgb(0.9, 0.25, 0.25)
        };

        let password = text_input("Password / PAM response", &self.password)
            .on_input(LockMessage::PasswordChanged)
            .on_submit(LockMessage::VerifyRequested)
            .padding(12)
            .size(20)
            .secure(true);

        let verify = if self.verifying {
            button("Verifying...")
        } else {
            button("Unlock").on_press(LockMessage::VerifyRequested)
        };

        let content = column![
            title,
            text(&self.status).size(24).color(status_color),
            text(format!("State: {state}    User: {}", self.username)).size(18),
            password,
            row![
                verify,
                button("Clear").on_press(LockMessage::BackspacePressed),
            ]
            .spacing(12),
            text("Enter = verify    Backspace/Clear = clear input").size(14),
        ]
        .spacing(18)
        .align_x(Horizontal::Center);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .into()
    }
}

fn pam_message_stream(
    receiver: &PamReceiver,
) -> iced::futures::stream::BoxStream<'static, LockMessage> {
    let receiver = receiver.clone();

    Box::pin(iced::stream::channel(10, async move |mut output| {
        loop {
            let message = receiver
                .0
                .lock()
                .ok()
                .and_then(|receiver| receiver.recv().ok())
                .unwrap_or_else(|| "PAM message channel closed".to_owned());

            if output.send(LockMessage::PamMessage(message)).await.is_err() {
                break;
            }
        }
    }))
}

fn verify_request(
    runtime: Arc<Runtime>,
    username: String,
    password: String,
    tty: Option<String>,
) -> std::result::Result<String, String> {
    let mut request = AuthRequest {
        username,
        password,
        tty,
    };

    let outcome = runtime.authenticate(&request);
    request.clear_secret();
    outcome
        .map(|success| success.username)
        .map_err(|reason| reason.to_string())
}

fn prompt_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    read_line_chomped()
}

fn prompt_secret(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;

    let echo_guard = if io::stdin().is_terminal() {
        EchoGuard::disable().ok()
    } else {
        None
    };
    let line = read_line_chomped();
    if echo_guard.is_some() {
        println!();
    }
    line
}

fn read_line_chomped() -> io::Result<String> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

struct EchoGuard;

impl EchoGuard {
    fn disable() -> io::Result<Self> {
        let status = Command::new("stty").arg("-echo").status()?;
        if status.success() {
            Ok(Self)
        } else {
            Err(io::Error::other("stty -echo failed"))
        }
    }
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        let _ = Command::new("stty").arg("echo").status();
    }
}

fn print_help() {
    println!(
        "Usage: limes-frontend-native [login|lock]\n\n\
Starter text frontend that links directly to limes-core. Replace this crate with\n\
a real native renderer or webview frontend as the project matures."
    );
}
