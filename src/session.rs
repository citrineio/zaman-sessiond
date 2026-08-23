use crate::command::CommandSpec;
use crate::error::{message, Result};
use crate::inputplumber::{InputPlumber, Inventory, INTERCEPT_NONE, INTERCEPT_PASS};
use crate::systemd::UserSystemd;
use std::fmt;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl fmt::Display for SessionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOutcome {
    Guide { target: String },
    GameExited,
    StopRequested,
}

impl fmt::Display for SessionOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Guide { target } => write!(formatter, "Guide request from {target}"),
            Self::GameExited => write!(formatter, "game exited normally"),
            Self::StopRequested => write!(formatter, "stop requested by the session service"),
        }
    }
}

pub struct Session {
    input: InputPlumber,
    systemd: UserSystemd,
    inventory: Inventory,
    state: SessionState,
}

impl Session {
    pub fn new(input: InputPlumber, systemd: UserSystemd, inventory: Inventory) -> Self {
        Self {
            input,
            systemd,
            inventory,
            state: SessionState::Idle,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub async fn run(
        &mut self,
        command: &CommandSpec,
        stop: watch::Receiver<bool>,
    ) -> Result<SessionOutcome> {
        // Reset stale interception before accepting a new session. Normal
        // completion and handled errors are also cleaned up below.
        self.input
            .set_intercept_mode(&self.inventory, INTERCEPT_NONE)
            .await?;

        self.transition(SessionState::Starting);

        let session_result = match self
            .input
            .set_intercept_mode(&self.inventory, INTERCEPT_PASS)
            .await
        {
            Ok(()) => self.run_active(command, stop).await,
            Err(error) => Err(error),
        };

        let cleanup_result = self
            .input
            .set_intercept_mode(&self.inventory, INTERCEPT_NONE)
            .await;
        let result = combine_cleanup(session_result, cleanup_result);

        if result.is_ok() {
            self.transition(SessionState::Idle);
        } else {
            self.transition(SessionState::Failed);
        }

        result
    }

    async fn run_active(
        &mut self,
        command: &CommandSpec,
        mut stop: watch::Receiver<bool>,
    ) -> Result<SessionOutcome> {
        let game = self.systemd.launch(command).await?;
        self.transition(SessionState::Running);
        println!("Guide, D-Bus Stop, and natural game exit are now supervised.");

        let event_result = {
            tokio::select! {
                guide = self.input.wait_for_guide(&self.inventory) => {
                    guide.map(|target| SessionOutcome::Guide { target })
                }
                game_exit = self.systemd.wait_for_exit(&game) => {
                    game_exit.map(|()| SessionOutcome::GameExited)
                }
                stop_request = wait_for_stop(&mut stop) => {
                    stop_request.map(|()| SessionOutcome::StopRequested)
                }
            }
        };

        self.transition(SessionState::Stopping);

        let stop_result = match &event_result {
            Ok(SessionOutcome::GameExited) => Ok(()),
            _ => self.systemd.stop(&game).await,
        };

        combine_stop(event_result, stop_result)
    }

    fn transition(&mut self, state: SessionState) {
        println!("Session state: {} -> {state}", self.state);
        self.state = state;
    }
}

async fn wait_for_stop(stop: &mut watch::Receiver<bool>) -> Result<()> {
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        stop.changed()
            .await
            .map_err(|_| message("session stop channel closed"))?;
    }
}

fn combine_stop(
    event_result: Result<SessionOutcome>,
    stop_result: Result<()>,
) -> Result<SessionOutcome> {
    match (event_result, stop_result) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(event_error), Ok(())) => Err(event_error),
        (Ok(_), Err(stop_error)) => Err(stop_error),
        (Err(event_error), Err(stop_error)) => Err(message(format!(
            "session event failed: {event_error}; game stop also failed: {stop_error}"
        ))),
    }
}

fn combine_cleanup(
    session_result: Result<SessionOutcome>,
    cleanup_result: Result<()>,
) -> Result<SessionOutcome> {
    match (session_result, cleanup_result) {
        (Ok(outcome), Ok(())) => Ok(outcome),
        (Err(session_error), Ok(())) => Err(session_error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(session_error), Err(cleanup_error)) => Err(message(format!(
            "session failed: {session_error}; input cleanup also failed: {cleanup_error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::SessionState;

    #[test]
    fn idle_is_the_initial_public_state() {
        assert_eq!(SessionState::Idle.to_string(), "Idle");
    }
}
