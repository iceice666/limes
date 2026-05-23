use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::{self, Command};

use limes_core::{Config, FrontendSpec, LimesError, Result, Runtime};
use limes_proto::AuthRequest;

fn main() {
    if let Err(error) = run() {
        eprintln!("limes: {error}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("login") => login(&args[1..]),
        Some("lock") => lock(&args[1..]),
        Some("help" | "--help" | "-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => Err(LimesError::Config(format!(
            "unknown command `{other}`; try `limes --help`"
        ))),
    }
}

fn login(args: &[String]) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        print_login_help();
        return Ok(());
    }

    let mut config = Config::from_env()?;
    apply_login_args(&mut config, args)?;
    let runtime = Runtime::from_config(config)?;

    match &runtime.config().login_frontend {
        FrontendSpec::BuiltinText => run_builtin_login(&runtime),
        FrontendSpec::External { .. } => {
            let code = runtime.launch_login_frontend()?;
            if code == 0 {
                Ok(())
            } else {
                Err(LimesError::Frontend(format!(
                    "login frontend exited with code {code}"
                )))
            }
        }
    }
}

fn lock(args: &[String]) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        print_lock_help();
        return Ok(());
    }
    if !args.is_empty() {
        return Err(LimesError::Config(format!(
            "unexpected lock arguments: {}",
            args.join(" ")
        )));
    }

    let runtime = Runtime::from_env()?;
    runtime.lock_now()?;
    match runtime.launch_lock_frontend()? {
        Some(0) | None => {
            println!("locked");
            Ok(())
        }
        Some(code) => Err(LimesError::Frontend(format!(
            "lock frontend exited with code {code}"
        ))),
    }
}

fn apply_login_args(config: &mut Config, args: &[String]) -> Result<()> {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--builtin" => {
                config.login_frontend = FrontendSpec::BuiltinText;
                i += 1;
            }
            "--frontend" => {
                let program = args.get(i + 1).ok_or_else(|| {
                    LimesError::Config("--frontend requires a program path".to_owned())
                })?;
                let mut frontend_args = args.get(i + 2..).unwrap_or_default().to_vec();
                if frontend_args.first().is_some_and(|arg| arg == "--") {
                    frontend_args.remove(0);
                }
                config.login_frontend = FrontendSpec::external(program.clone(), frontend_args);
                return Ok(());
            }
            other => {
                return Err(LimesError::Config(format!(
                    "unknown login argument `{other}`; try `limes login --help`"
                )));
            }
        }
    }
    Ok(())
}

fn run_builtin_login(runtime: &Runtime) -> Result<()> {
    eprintln!("limes login: builtin text frontend");
    eprintln!("note: this starter frontend is for development; use a real renderer for production");

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
                println!(
                    "started session for {} with pid {} ({})",
                    handle.username,
                    handle.pid,
                    handle.command.join(" ")
                );
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
        "limes - Log In Manager & Screenlock\n\n\
Usage:\n\
  limes login [--builtin | --frontend PROGRAM [-- ARGS...]]\n\
  limes lock\n\n\
Environment:\n\
  LIMES_AUTH_BACKEND=pam|dev|deny   (default: pam)\n\
  LIMES_DEV_PASSWORD=secret         required for dev backend\n\
  LIMES_LOGIN_FRONTEND='cmd args'   default login frontend\n\
  LIMES_LOCK_FRONTEND='cmd args'    optional lock frontend\n\
  LIMES_SESSION_COMMAND='cmd args'  command after successful login\n\
  LIMES_LOG_EVENTS=1                log backend events to stderr"
    );
}

fn print_login_help() {
    println!(
        "Usage: limes login [--builtin | --frontend PROGRAM [-- ARGS...]]\n\n\
Called after boot by a service/DM unit. By default it hosts the builtin text\n\
frontend. External webview/native frontends can be launched with --frontend and\n\
may link to limes-core for authentication/session work."
    );
}

fn print_lock_help() {
    println!(
        "Usage: limes lock\n\n\
Locks the current session through limes-core. Configure LIMES_LOCK_FRONTEND to\n\
start a renderer for the locked UI."
    );
}
