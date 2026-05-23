use std::process::{Command, ExitStatus};

use limes_proto::LimesEvent;

use crate::config::FrontendSpec;
use crate::error::{LimesError, Result};
use crate::events::EventBus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendMode {
    Login,
    Lock,
}

impl FrontendMode {
    #[must_use]
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Lock => "lock",
        }
    }
}

#[derive(Clone)]
pub struct FrontendRunner {
    events: EventBus,
}

impl FrontendRunner {
    #[must_use]
    pub fn new(events: EventBus) -> Self {
        Self { events }
    }

    pub fn run(&self, spec: &FrontendSpec, mode: FrontendMode) -> Result<ExitStatus> {
        let program = &spec.program;
        let args = &spec.args;

        let mut rendered = Vec::with_capacity(args.len() + 1);
        rendered.push(program.clone());
        rendered.extend(args.iter().cloned());
        self.events.emit(LimesEvent::FrontendStarted {
            mode: mode.as_arg().to_owned(),
            command: rendered,
        });

        Command::new(program)
            .args(args)
            .arg(mode.as_arg())
            .status()
            .map_err(|error| {
                LimesError::Frontend(format!("failed to start frontend `{program}`: {error}"))
            })
    }
}
