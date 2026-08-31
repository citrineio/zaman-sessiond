use crate::command::CommandSpec;
use crate::error::{message, Result};
use crate::guide::GuideAction;
use crate::inputplumber::{
    InputPlumber, Inventory, SystemInputEvent, INTERCEPT_ALL, INTERCEPT_NONE, INTERCEPT_PASS,
};
use crate::menu::MenuController;
use crate::systemd::UserSystemd;
use std::fmt;
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    MenuOpen,
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
    GameExited,
    StopRequested,
    ExitGameRequested,
    ShutdownRequested,
}

impl fmt::Display for SessionOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GameExited => write!(formatter, "game exited normally"),
            Self::StopRequested => write!(formatter, "stop requested by the session service"),
            Self::ExitGameRequested => write!(formatter, "exit requested from the system menu"),
            Self::ShutdownRequested => {
                write!(formatter, "shutdown requested from the system menu")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControl {
    Stop,
    Resume,
    ExitGame,
    Shutdown,
}

pub struct Session {
    input: InputPlumber,
    systemd: UserSystemd,
    inventory: Inventory,
    menu: MenuController,
    state: SessionState,
}

impl Session {
    pub fn new(
        input: InputPlumber,
        systemd: UserSystemd,
        inventory: Inventory,
        menu: MenuController,
    ) -> Self {
        Self {
            input,
            systemd,
            inventory,
            menu,
            state: SessionState::Idle,
        }
    }

    pub async fn run(
        &mut self,
        command: &CommandSpec,
        controls: mpsc::Receiver<SessionControl>,
    ) -> Result<SessionOutcome> {
        self.menu.close("session-reset");

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
            Ok(()) => self.run_active(command, controls).await,
            Err(error) => Err(error),
        };

        self.menu.close("session-ended");
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
        mut controls: mpsc::Receiver<SessionControl>,
    ) -> Result<SessionOutcome> {
        let game = self.systemd.launch(command).await?;
        self.transition(SessionState::Running);
        println!("Guide menu requests, D-Bus Stop, and natural game exit are now supervised.");

        let mut input_monitor = self.input.monitor_system_input(&self.inventory);

        let event_result: Result<SessionOutcome> = loop {
            tokio::select! {
                input = input_monitor.next() => {
                    self.handle_system_input(input).await?;
                }
                game_exit = self.systemd.wait_for_exit(&game) => {
                    break game_exit.map(|()| SessionOutcome::GameExited);
                }
                control = controls.recv() => {
                    match control {
                        Some(SessionControl::Resume) => self.resume_menu().await?,
                        Some(SessionControl::ExitGame) => {
                            self.menu.close("exit-game");
                            break Ok(SessionOutcome::ExitGameRequested);
                        }
                        Some(SessionControl::Shutdown) => {
                            self.menu.close("shutdown");
                            break Ok(SessionOutcome::ShutdownRequested);
                        }
                        Some(SessionControl::Stop) | None => {
                            self.menu.close("session-stop");
                            break Ok(SessionOutcome::StopRequested);
                        }
                    }
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

    async fn handle_system_input(&mut self, event: SystemInputEvent) -> Result<()> {
        match event {
            SystemInputEvent::Guide { target, action } => match action {
                GuideAction::MenuRequested => {
                    if self.menu.snapshot().open {
                        println!(
                            "Guide press from {target} ignored because the system menu is open."
                        );
                        return Ok(());
                    }

                    self.input
                        .set_intercept_mode(&self.inventory, INTERCEPT_ALL)
                        .await?;
                    if self.menu.open("guide") {
                        self.transition(SessionState::MenuOpen);
                        println!("Guide press requested the system menu from {target}.");
                    }
                }
                GuideAction::Released => {
                    println!("Guide released by {target}; menu lifecycle retains input ownership.");
                }
            },
            SystemInputEvent::MenuInput { event, value } => {
                let forwarded = self.menu.input(event.clone(), value);
                if forwarded && value > 0.5 {
                    println!("System menu input event={event} value={value}.");
                }
            }
        }

        Ok(())
    }

    async fn resume_menu(&mut self) -> Result<()> {
        if !self.menu.snapshot().open {
            println!("Resume ignored because the system menu is already closed.");
            return Ok(());
        }

        self.input
            .set_intercept_mode(&self.inventory, INTERCEPT_PASS)
            .await?;
        self.menu.close("resume");
        self.transition(SessionState::Running);
        println!("System menu resumed the game.");
        Ok(())
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
