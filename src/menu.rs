use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use zaman_sessiond::contract::MenuContextTuple;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Foreground {
    Library,
    Game,
    Menu,
    Transfer,
}

impl Foreground {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Game => "game",
            Self::Menu => "menu",
            Self::Transfer => "transfer",
        }
    }
}

impl fmt::Display for Foreground {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnTarget {
    Library,
    Game,
}

impl ReturnTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Game => "game",
        }
    }
}

impl fmt::Display for ReturnTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuSnapshot {
    pub open: bool,
    pub generation: u64,
    pub reason: String,
    pub foreground: Foreground,
    pub return_target: Option<ReturnTarget>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEvent {
    Opened(MenuSnapshot),
    Closed(MenuSnapshot),
    ContextChanged(u64),
    TransferInput { generation: u64, event: String, value: f64 },
    Input {
        generation: u64,
        event: String,
        value: f64,
    },
}

#[derive(Debug)]
struct MenuState {
    transfer_generation: u64,
    transfer_phase: String,
    transfer_owner: Option<String>,
    transfer_menu: Option<u64>,
    error: String,
    generation: u64,
    reason: String,
    foreground: Foreground,
    return_target: Option<ReturnTarget>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            transfer_generation: 0,
            transfer_phase: "idle".into(),
            transfer_owner: None,
            transfer_menu: None,
            error: String::new(),
            generation: 0,
            reason: String::new(),
            foreground: Foreground::Library,
            return_target: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MenuController {
    state: Arc<Mutex<MenuState>>,
    events: mpsc::UnboundedSender<MenuEvent>,
    presented: watch::Sender<u64>,
    transfer_changed: watch::Sender<u64>,
}

impl MenuController {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MenuEvent>) {
        let (events, receiver) = mpsc::unbounded_channel();
        let (presented, _) = watch::channel(0);
        let (transfer_changed, _) = watch::channel(0);
        (
            Self {
                state: Arc::new(Mutex::new(MenuState::default())),
                events,
                presented,
                transfer_changed,
            },
            receiver,
        )
    }

    pub fn snapshot(&self) -> MenuSnapshot {
        snapshot(&self.lock())
    }

    /// Begins one menu transition. The worker serializes callers, while this
    /// lock makes the observable state and emitted generation indivisible.
    pub fn open(&self, reason: impl Into<String>) -> Option<u64> {
        let event = {
            let mut state = self.lock();
            if matches!(state.foreground, Foreground::Menu | Foreground::Transfer) {
                return None;
            }

            let previous = state.foreground;
            let target = match previous {
                Foreground::Library => ReturnTarget::Library,
                Foreground::Game => ReturnTarget::Game,
                Foreground::Menu | Foreground::Transfer => unreachable!(),
            };
            state.foreground = Foreground::Menu;
            state.return_target = Some(target);
            state.generation = state.generation.wrapping_add(1).max(1);
            state.reason = reason.into();
            state.error.clear();
            let snapshot = snapshot(&state);
            println!("foreground {previous} -> menu");
            println!("menu return_target={target}");
            MenuEvent::Opened(snapshot)
        };

        let generation = match &event {
            MenuEvent::Opened(snapshot) => snapshot.generation,
            _ => unreachable!(),
        };
        let _ = self.events.send(event);
        Some(generation)
    }

    pub fn close(&self, reason: impl Into<String>, game_active: bool) -> bool {
        let event = {
            let mut state = self.lock();
            if state.foreground != Foreground::Menu {
                return false;
            }

            let target = match state.return_target {
                Some(ReturnTarget::Game) if game_active => Foreground::Game,
                _ => Foreground::Library,
            };
            state.foreground = target;
            state.return_target = None;
            state.reason = reason.into();
            println!("foreground menu -> {target}");
            MenuEvent::Closed(snapshot(&state))
        };

        let _ = self.events.send(event);
        true
    }

    pub fn game_started(&self) {
        let mut state = self.lock();
        if state.foreground == Foreground::Library {
            println!("foreground library -> game");
            state.foreground = Foreground::Game;
            state.reason = "game-started".to_string();
        }
    }

    /// Returns true when ending the game also closes a menu that was meant to
    /// return to it. This deterministic fallback reveals Pegasus.
    pub fn game_ended(&self, reason: impl Into<String>) -> bool {
        let event = {
            let mut state = self.lock();
            let reason = reason.into();
            match (state.foreground, state.return_target) {
                (Foreground::Menu, Some(ReturnTarget::Game)) => {
                    state.foreground = Foreground::Library;
                    state.return_target = None;
                    state.reason = reason;
                    println!("game exited while menu was open; restoring library");
                    Some(MenuEvent::Closed(snapshot(&state)))
                }
                (Foreground::Game, _) => {
                    println!("foreground game -> library");
                    state.foreground = Foreground::Library;
                    state.return_target = None;
                    state.reason = reason;
                    None
                }
                _ => None,
            }
        };

        if let Some(event) = event {
            let _ = self.events.send(event);
            true
        } else {
            false
        }
    }

    pub fn input(&self, event: impl Into<String>, value: f64) -> bool {
        let generation = {
            let state = self.lock();
            if state.foreground != Foreground::Menu {
                return false;
            }
            state.generation
        };

        let _ = self.events.send(MenuEvent::Input {
            generation,
            event: event.into(),
            value,
        });
        true
    }

    pub fn presented(&self, generation: u64) -> bool {
        let state = self.lock();
        if state.foreground != Foreground::Menu || state.generation != generation {
            return false;
        }
        drop(state);
        self.presented.send_replace(generation);
        true
    }

    pub async fn wait_until_presented(&self, generation: u64, duration: Duration) -> bool {
        let mut presented = self.presented.subscribe();
        let wait = async {
            loop {
                if *presented.borrow_and_update() == generation {
                    return true;
                }
                if presented.changed().await.is_err() {
                    return false;
                }
            }
        };
        timeout(duration, wait).await.unwrap_or(false)
    }

    /// Derive the current menu context for the renderer.
    /// Keep the originating menu mapped while the transfer lease makes it pending.
    pub fn context(&self) -> MenuContextTuple {
        let state = self.lock();
        let open = state.foreground == Foreground::Menu || state.transfer_menu.is_some();
        let return_target = state
            .return_target
            .map(|t| t.as_str().to_string())
            .unwrap_or_default();
        let game_state = if state.return_target == Some(ReturnTarget::Game) {
            "paused"
        } else {
            "idle"
        }
        .to_string();
        let mut actions = Vec::new();
        if open {
            actions.push("resume".to_string());
            if state.return_target == Some(ReturnTarget::Game) {
                actions.push("exit-game".to_string());
            } else {
                actions.push("transfer".to_string());
            }
            actions.push("reboot".to_string());
            actions.push("shutdown".to_string());
        }
        (
            open,
            state.generation,
            return_target,
            game_state,
            state.foreground == Foreground::Transfer,
            state.error.clone(),
            actions,
        )
    }

    pub fn transfer_busy(&self) -> bool {
        self.lock().foreground == Foreground::Transfer
    }

    pub fn transfer_context(&self) -> (u64, String) {
        let state = self.lock();
        (state.transfer_generation, state.transfer_phase.clone())
    }

    pub fn transfer_changes(&self) -> watch::Receiver<u64> {
        self.transfer_changed.subscribe()
    }

    pub fn reserve_transfer(&self, origin: u64, game_active: bool) -> Result<u64, String> {
        let mut state = self.lock();
        if game_active || state.foreground == Foreground::Game
            || state.return_target == Some(ReturnTarget::Game) {
            return Err("Transfer Games is only available from the library".into());
        }
        if state.foreground == Foreground::Transfer {
            return Err("busy: Transfer Games owns the foreground".into());
        }
        let valid = if origin == 0 {
            state.foreground == Foreground::Library
        } else {
            state.foreground == Foreground::Menu && state.generation == origin
        };
        if !valid { return Err("stale or unavailable library menu context".into()); }
        state.transfer_generation = state.transfer_generation.wrapping_add(1).max(1);
        state.transfer_menu = (origin != 0).then_some(origin);
        state.transfer_owner = None;
        state.transfer_phase = "starting".into();
        state.foreground = Foreground::Transfer;
        state.error.clear();
        let generation = state.transfer_generation;
        let _ = self.events.send(MenuEvent::ContextChanged(state.generation));
        self.transfer_changed.send_modify(|revision| *revision += 1);
        Ok(generation)
    }

    pub fn transfer_presented(&self, generation: u64, owner: &str) -> bool {
        let mut state = self.lock();
        if state.foreground != Foreground::Transfer || state.transfer_generation != generation
            || state.transfer_phase != "starting" || owner.is_empty() { return false; }
        state.transfer_owner = Some(owner.into());
        state.transfer_phase = "active".into();
        self.transfer_changed.send_modify(|revision| *revision += 1);
        true
    }

    pub fn transfer_owner(&self) -> Option<String> { self.lock().transfer_owner.clone() }

    pub fn stop_transfer(&self, generation: u64) -> bool {
        let mut state = self.lock();
        if state.foreground != Foreground::Transfer || state.transfer_generation != generation {
            return false;
        }
        state.transfer_phase = "stopping".into();
        self.transfer_changed.send_modify(|revision| *revision += 1);
        true
    }

    pub fn transfer_menu_lost(&self) {
        self.lock().transfer_menu = None;
    }

    pub fn transfer_cleanup_failed(&self, generation: u64, error: String) {
        let mut state = self.lock();
        if state.foreground == Foreground::Transfer && state.transfer_generation == generation {
            state.error = error;
            let _ = self.events.send(MenuEvent::ContextChanged(state.generation));
        }
    }

    pub fn finish_transfer(&self, generation: u64, error: Option<String>) -> bool {
        let mut state = self.lock();
        if state.foreground != Foreground::Transfer || state.transfer_generation != generation {
            return false;
        }
        let restore = state.transfer_menu == Some(state.generation);
        state.foreground = if restore { Foreground::Menu } else { Foreground::Library };
        if !restore { state.return_target = None; }
        state.transfer_menu = None;
        state.transfer_owner = None;
        state.transfer_phase = if error.is_some() { "failed" } else { "idle" }.into();
        state.error = error.unwrap_or_default();
        state.reason = "transfer-ended".into();
        let _ = self.events.send(MenuEvent::ContextChanged(state.generation));
        self.transfer_changed.send_modify(|revision| *revision += 1);
        true
    }

    pub fn transfer_input(&self, event: String, value: f64) -> bool {
        let state = self.lock();
        if state.foreground != Foreground::Transfer || state.transfer_phase != "active" {
            return false;
        }
        if !matches!(event.as_str(), "ui_up" | "ui_down" | "ui_left" | "ui_right" |
            "ui_accept" | "ui_back" | "ui_guide") { return false; }
        let _ = self.events.send(MenuEvent::TransferInput {
            generation: state.transfer_generation, event, value,
        });
        true
    }

    fn lock(&self) -> MutexGuard<'_, MenuState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn snapshot(state: &MenuState) -> MenuSnapshot {
    MenuSnapshot {
        open: state.foreground == Foreground::Menu || state.transfer_menu.is_some(),
        generation: state.generation,
        reason: state.reason.clone(),
        foreground: state.foreground,
        return_target: state.return_target,
    }
}

#[cfg(test)]
mod tests {
    use super::{Foreground, MenuController, MenuEvent, ReturnTarget};

    #[test]
    fn library_menu_library() {
        let (menu, _) = MenuController::new();
        assert_eq!(menu.snapshot().foreground, Foreground::Library);
        assert_eq!(menu.open("manual"), Some(1));
        assert_eq!(menu.snapshot().return_target, Some(ReturnTarget::Library));
        assert!(menu.close("resume", false));
        assert_eq!(menu.snapshot().foreground, Foreground::Library);
        assert_eq!(menu.snapshot().return_target, None);
    }

    #[test]
    fn game_menu_game() {
        let (menu, _) = MenuController::new();
        menu.game_started();
        assert_eq!(menu.open("manual"), Some(1));
        assert_eq!(menu.snapshot().return_target, Some(ReturnTarget::Game));
        assert!(menu.close("resume", true));
        assert_eq!(menu.snapshot().foreground, Foreground::Game);
    }

    #[test]
    fn duplicate_open_does_not_create_duplicate_state() {
        let (menu, mut events) = MenuController::new();
        assert_eq!(menu.open("first"), Some(1));
        assert_eq!(menu.open("duplicate"), None);
        assert_eq!(menu.snapshot().generation, 1);
        assert!(matches!(events.try_recv(), Ok(MenuEvent::Opened(_))));
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn game_exit_while_menu_is_open_restores_library() {
        let (menu, mut events) = MenuController::new();
        menu.game_started();
        menu.open("manual");
        let _ = events.try_recv();
        assert!(menu.game_ended("game-exited"));
        assert_eq!(menu.snapshot().foreground, Foreground::Library);
        assert_eq!(menu.snapshot().return_target, None);
        assert!(matches!(events.try_recv(), Ok(MenuEvent::Closed(_))));
    }

    #[test]
    fn failed_menu_presentation_restores_previous_foreground() {
        let (menu, _) = MenuController::new();
        menu.game_started();
        menu.open("manual");
        assert!(menu.close("menu-launch-failed", true));
        assert_eq!(menu.snapshot().foreground, Foreground::Game);
    }

    #[test]
    fn closing_an_already_closed_menu_is_safe() {
        let (menu, _) = MenuController::new();
        assert!(!menu.close("duplicate", false));
        assert_eq!(menu.snapshot().foreground, Foreground::Library);
    }

    #[test]
    fn context_lists_resume_reboot_and_shutdown_in_library() {
        let (menu, _) = MenuController::new();
        menu.open("manual");
        let context = menu.context();
        assert!(context.0);
        assert_eq!(context.2, "library");
        assert_eq!(context.3, "idle");
        assert_eq!(context.6, vec!["resume", "transfer", "reboot", "shutdown"]);
    }

    #[test]
    fn context_lists_resume_exit_game_reboot_and_shutdown_in_game() {
        let (menu, _) = MenuController::new();
        menu.game_started();
        menu.open("manual");
        let context = menu.context();
        assert_eq!(context.2, "game");
        assert_eq!(context.3, "paused");
        assert_eq!(context.6, vec!["resume", "exit-game", "reboot", "shutdown"]);
    }

    #[test]
    fn context_when_closed_has_no_allowed_actions() {
        let (menu, _) = MenuController::new();
        let context = menu.context();
        assert!(!context.0);
        assert!(context.6.is_empty());
    }
    #[test]
    fn transfer_library_only_and_stale_context_rejected() {
        let (menu, _) = MenuController::new();
        menu.open("test");
        assert!(menu.context().6.contains(&"transfer".to_string()));
        assert!(menu.reserve_transfer(99, false).is_err());
        assert!(menu.reserve_transfer(0, false).is_err());
        menu.close("test", false);
        menu.game_started();
        menu.open("test");
        assert!(!menu.context().6.contains(&"transfer".to_string()));
        assert!(menu.reserve_transfer(menu.context().1, true).is_err());
    }

    #[test]
    fn transfer_suspends_input_and_returns_to_same_menu_generation() {
        let (menu, _) = MenuController::new();
        let origin = menu.open("test").unwrap();
        let generation = menu.reserve_transfer(origin, false).unwrap();
        assert_eq!(menu.snapshot().foreground, Foreground::Transfer);
        assert!(menu.context().0);
        assert!(menu.context().4);
        assert!(!menu.input("ui_accept", 1.0));
        assert!(menu.reserve_transfer(origin, false).is_err());
        assert!(!menu.transfer_presented(generation + 1, ":1.2"));
        assert!(menu.transfer_presented(generation, ":1.2"));
        assert!(!menu.transfer_presented(generation, ":1.3"));
        assert!(!menu.finish_transfer(generation + 1, None));
        assert!(menu.finish_transfer(generation, None));
        assert_eq!(menu.context().1, origin);
        assert!(!menu.context().4);
        assert!(menu.input("ui_accept", 1.0));
    }

    #[test]
    fn transfer_direct_launch_and_menu_loss_return_to_library() {
        let (menu, _) = MenuController::new();
        let first = menu.reserve_transfer(0, false).unwrap();
        assert!(!menu.context().0);
        assert!(menu.finish_transfer(first, None));
        let origin = menu.open("test").unwrap();
        let second = menu.reserve_transfer(origin, false).unwrap();
        assert!(second > first);
        menu.transfer_menu_lost();
        assert!(menu.finish_transfer(second, Some("launch failed".into())));
        assert_eq!(menu.snapshot().foreground, Foreground::Library);
        assert!(!menu.transfer_presented(first, ":1.2"));
        assert!(!menu.finish_transfer(first, None));
    }

}
