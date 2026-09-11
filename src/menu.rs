use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Foreground {
    Library,
    Game,
    Menu,
}

impl Foreground {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Game => "game",
            Self::Menu => "menu",
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
    Input {
        generation: u64,
        event: String,
        value: f64,
    },
}

#[derive(Debug)]
struct MenuState {
    generation: u64,
    reason: String,
    foreground: Foreground,
    return_target: Option<ReturnTarget>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
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
}

impl MenuController {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MenuEvent>) {
        let (events, receiver) = mpsc::unbounded_channel();
        let (presented, _) = watch::channel(0);
        (
            Self {
                state: Arc::new(Mutex::new(MenuState::default())),
                events,
                presented,
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
            if state.foreground == Foreground::Menu {
                return None;
            }

            let previous = state.foreground;
            let target = match previous {
                Foreground::Library => ReturnTarget::Library,
                Foreground::Game => ReturnTarget::Game,
                Foreground::Menu => unreachable!(),
            };
            state.foreground = Foreground::Menu;
            state.return_target = Some(target);
            state.generation = state.generation.wrapping_add(1).max(1);
            state.reason = reason.into();
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

    fn lock(&self) -> MutexGuard<'_, MenuState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn snapshot(state: &MenuState) -> MenuSnapshot {
    MenuSnapshot {
        open: state.foreground == Foreground::Menu,
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
}
