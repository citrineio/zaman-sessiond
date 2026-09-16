use crate::command::CommandSpec;
use crate::error::{message, Result};
use crate::systemd::UserSystemd;
use std::fmt;
use tokio::sync::mpsc;

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
        write!(formatter, "{self:?}")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionOutcome {
    GameExited,
    StopRequested,
    ExitGameRequested,
    ShutdownRequested,
    RebootRequested,
}

impl fmt::Display for SessionOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GameExited => write!(formatter, "game exited normally"),
            Self::StopRequested => write!(formatter, "stop requested by the session service"),
            Self::ExitGameRequested => write!(formatter, "exit requested from the system menu"),
            Self::RebootRequested => write!(formatter, "reboot requested from the system menu"),
            Self::ShutdownRequested => {
                write!(formatter, "shutdown requested from the system menu")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControl {
    Stop,
    ExitGame,
    Shutdown,
    Reboot,
}

impl SessionControl {
    fn outcome(self) -> SessionOutcome {
        match self {
            Self::Stop => SessionOutcome::StopRequested,
            Self::ExitGame => SessionOutcome::ExitGameRequested,
            Self::Shutdown => SessionOutcome::ShutdownRequested,
            Self::Reboot => SessionOutcome::RebootRequested,
        }
    }
}

pub struct Session {
    systemd: UserSystemd,
    state: SessionState,
}

impl Session {
    pub fn new(systemd: UserSystemd) -> Self {
        Self {
            systemd,
            state: SessionState::Idle,
        }
    }

    pub async fn run(
        &mut self,
        command: &CommandSpec,
        mut controls: mpsc::Receiver<SessionControl>,
    ) -> Result<SessionOutcome> {
        self.transition(SessionState::Starting);
        let game = match self.systemd.launch(command).await {
            Ok(game) => game,
            Err(error) => {
                self.transition(SessionState::Failed);
                return Err(error);
            }
        };

        self.transition(SessionState::Running);
        println!("D-Bus Stop and natural game exit are now supervised.");

        let event_result: Result<SessionOutcome> = loop {
            tokio::select! {
                game_exit = self.systemd.wait_for_exit(&game) => {
                    break game_exit.map(|()| SessionOutcome::GameExited);
                }
                control = controls.recv() => {
                    break Ok(control.unwrap_or(SessionControl::Stop).outcome());
                }
            }
        };

        self.transition(SessionState::Stopping);
        let stop_result = match &event_result {
            Ok(SessionOutcome::GameExited) => Ok(()),
            _ => self.systemd.stop(&game).await,
        };
        let result = combine_stop(event_result, stop_result);
        self.transition(if result.is_ok() {
            SessionState::Idle
        } else {
            SessionState::Failed
        });
        result
    }

    fn transition(&mut self, state: SessionState) {
        println!("Session state: {} -> {state}", self.state);
        self.state = state;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_the_initial_public_state() {
        assert_eq!(SessionState::Idle.to_string(), "Idle");
    }

    #[test]
    fn reboot_requires_a_successful_game_stop() {
        assert_eq!(
            combine_stop(Ok(SessionOutcome::RebootRequested), Ok(())).unwrap(),
            SessionOutcome::RebootRequested
        );
        assert!(combine_stop(
            Ok(SessionOutcome::RebootRequested),
            Err(message("stop failed"))
        )
        .is_err());
        assert!(combine_stop(Err(message("event failed")), Ok(())).is_err());
    }

    #[test]
    fn reboot_control_selects_reboot_outcome() {
        assert_eq!(
            SessionControl::Reboot.outcome(),
            SessionOutcome::RebootRequested
        );
        assert_eq!(
            SessionControl::Shutdown.outcome(),
            SessionOutcome::ShutdownRequested
        );
        assert_eq!(
            SessionControl::ExitGame.outcome(),
            SessionOutcome::ExitGameRequested
        );
        assert_eq!(
            SessionControl::Stop.outcome(),
            SessionOutcome::StopRequested
        );
    }
}
