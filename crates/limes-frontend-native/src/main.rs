use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::{self, Command};

use limes_core::{LimesError, Result, Runtime};
use limes_proto::AuthRequest;

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
    let runtime = Runtime::from_env()?;
    runtime.lock_now()?;
    println!("locked (starter text frontend)");
    Ok(())
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
