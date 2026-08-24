use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuSnapshot {
    pub open: bool,
    pub generation: u64,
    pub reason: String,
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

#[derive(Debug, Default)]
struct MenuState {
    open: bool,
    generation: u64,
    reason: String,
}

#[derive(Clone, Debug)]
pub struct MenuController {
    state: Arc<Mutex<MenuState>>,
    events: mpsc::UnboundedSender<MenuEvent>,
}

impl MenuController {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MenuEvent>) {
        let (events, receiver) = mpsc::unbounded_channel();
        (
            Self {
                state: Arc::new(Mutex::new(MenuState::default())),
                events,
            },
            receiver,
        )
    }

    pub fn snapshot(&self) -> MenuSnapshot {
        snapshot(&self.lock())
    }

    pub fn open(&self, reason: impl Into<String>) -> bool {
        let event = {
            let mut state = self.lock();
            if state.open {
                return false;
            }

            state.open = true;
            state.generation = state.generation.wrapping_add(1).max(1);
            state.reason = reason.into();
            MenuEvent::Opened(snapshot(&state))
        };

        let _ = self.events.send(event);
        true
    }

    pub fn close(&self, reason: impl Into<String>) -> bool {
        let event = {
            let mut state = self.lock();
            if !state.open {
                return false;
            }

            state.open = false;
            state.reason = reason.into();
            MenuEvent::Closed(snapshot(&state))
        };

        let _ = self.events.send(event);
        true
    }

    pub fn input(&self, event: impl Into<String>, value: f64) -> bool {
        let generation = {
            let state = self.lock();
            if !state.open {
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

    fn lock(&self) -> MutexGuard<'_, MenuState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn snapshot(state: &MenuState) -> MenuSnapshot {
    MenuSnapshot {
        open: state.open,
        generation: state.generation,
        reason: state.reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MenuController, MenuEvent};

    #[test]
    fn opening_is_idempotent_and_advances_the_generation() {
        let (menu, mut events) = MenuController::new();

        assert!(menu.open("guide"));
        assert!(!menu.open("duplicate"));
        assert_eq!(menu.snapshot().generation, 1);
        assert!(menu.snapshot().open);

        let MenuEvent::Opened(snapshot) = events.try_recv().expect("open event") else {
            panic!("expected open event");
        };
        assert_eq!(snapshot.reason, "guide");
        assert!(events.try_recv().is_err());

        assert!(menu.close("resume"));
        assert!(menu.open("guide"));
        assert_eq!(menu.snapshot().generation, 2);
    }

    #[test]
    fn closing_is_idempotent_and_records_the_reason() {
        let (menu, mut events) = MenuController::new();
        menu.open("guide");
        let _ = events.try_recv();

        assert!(menu.close("resume"));
        assert!(!menu.close("duplicate"));
        assert!(!menu.snapshot().open);
        assert_eq!(menu.snapshot().reason, "resume");

        let MenuEvent::Closed(snapshot) = events.try_recv().expect("close event") else {
            panic!("expected close event");
        };
        assert_eq!(snapshot.generation, 1);
    }

    #[test]
    fn normalized_input_is_published_only_while_open() {
        let (menu, mut events) = MenuController::new();
        assert!(!menu.input("ui_accept", 1.0));

        menu.open("guide");
        let _ = events.try_recv();
        assert!(menu.input("ui_accept", 1.0));

        let MenuEvent::Input {
            generation,
            event,
            value,
        } = events.try_recv().expect("input event")
        else {
            panic!("expected input event");
        };
        assert_eq!(generation, 1);
        assert_eq!(event, "ui_accept");
        assert_eq!(value, 1.0);
    }
}
