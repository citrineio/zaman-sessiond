//! Bounded lifecycle transaction primitive.
//!
//! Only one lifecycle operation (launch, close-menu, launch, stop, etc.) may
//! be in flight at a time. Each operation carries an `OperationIdentity`
//! captured at authorization. When the operation completes, the caller must
//! present the same identity; if the menu generation changed while it was
//! running, the completion is rejected so the caller can reconcile external
//! state before starting something new.

use std::time::{Duration, Instant};

/// Snapshot of observable daemon context at authorization time.
///
/// Currently this only carries the menu generation. Future versions will
/// add the game's invocation identity so that a stop operation authorized
/// for game A cannot be accepted after game B has replaced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationIdentity {
    pub menu_generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    OpenMenu,
    CloseMenu,
    Launch,
    Stop,
    PowerOff,
    Reboot,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenMenu => "open-menu",
            Self::CloseMenu => "close-menu",
            Self::Launch => "launch",
            Self::Stop => "stop",
            Self::PowerOff => "power-off",
            Self::Reboot => "reboot",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationToken(pub u64);

#[derive(Debug, Eq, PartialEq)]
pub enum StartRejected {
    Busy { current: OperationKind },
    ShuttingDown,
}

#[derive(Debug, Eq, PartialEq)]
pub enum CompleteRejected {
    NotInFlight,
    StaleToken { in_flight: u64, presented: u64 },
    IdentityChanged,
}

struct InFlight {
    token: OperationToken,
    kind: OperationKind,
    identity: OperationIdentity,
    started: Instant,
    deadline: Instant,
}

pub struct OperationSlot {
    next_token: u64,
    in_flight: Option<InFlight>,
    shutting_down: bool,
}

impl Default for OperationSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationSlot {
    pub fn new() -> Self {
        Self {
            next_token: 1,
            in_flight: None,
            shutting_down: false,
        }
    }

    pub fn begin(
        &mut self,
        kind: OperationKind,
        identity: OperationIdentity,
        budget: Duration,
        now: Instant,
    ) -> Result<OperationToken, StartRejected> {
        if self.shutting_down {
            return Err(StartRejected::ShuttingDown);
        }
        if let Some(current) = self.in_flight.as_ref() {
            return Err(StartRejected::Busy {
                current: current.kind,
            });
        }
        let token = OperationToken(self.next_token);
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.in_flight = Some(InFlight {
            token,
            kind,
            identity,
            started: now,
            deadline: now + budget,
        });
        Ok(token)
    }

    pub fn complete(
        &mut self,
        token: OperationToken,
        observed: OperationIdentity,
    ) -> Result<OperationKind, CompleteRejected> {
        let current = self
            .in_flight
            .as_ref()
            .ok_or(CompleteRejected::NotInFlight)?;
        if current.token != token {
            return Err(CompleteRejected::StaleToken {
                in_flight: current.token.0,
                presented: token.0,
            });
        }
        if current.identity != observed {
            return Err(CompleteRejected::IdentityChanged);
        }
        Ok(self.in_flight.take().expect("checked").kind)
    }

    /// Priority control path for Quit and unexpected loss. Does not validate
    /// identity; the caller is responsible for reconciling external state
    /// before the next `begin`.
    pub fn abort_for_quit(&mut self) -> Option<OperationKind> {
        self.in_flight.take().map(|f| f.kind)
    }

    /// Polled by the worker. Returns the abandoned kind if the deadline
    /// elapsed; the slot is cleared so the caller can reconcile.
    pub fn check_deadline(&mut self, now: Instant) -> Option<OperationKind> {
        let expired = self.in_flight.as_ref().is_some_and(|f| f.deadline <= now);
        if expired {
            self.in_flight.take().map(|f| f.kind)
        } else {
            None
        }
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn current_kind(&self) -> Option<OperationKind> {
        self.in_flight.as_ref().map(|f| f.kind)
    }

    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.in_flight
            .as_ref()
            .map(|f| now.saturating_duration_since(f.started))
    }

    pub fn begin_shutdown(&mut self) {
        self.shutting_down = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn ident(gen: u64) -> OperationIdentity {
        OperationIdentity {
            menu_generation: gen,
        }
    }

    #[test]
    fn new_slot_is_idle() {
        let slot = OperationSlot::new();
        assert!(!slot.is_busy());
        assert_eq!(slot.current_kind(), None);
    }

    #[test]
    fn begin_then_complete_round_trips() {
        let mut slot = OperationSlot::new();
        let now = Instant::now();
        let id = ident(1);
        let token = slot
            .begin(OperationKind::OpenMenu, id, Duration::from_secs(3), now)
            .expect("begin");
        assert!(slot.is_busy());
        assert_eq!(slot.current_kind(), Some(OperationKind::OpenMenu));
        let kind = slot.complete(token, id).expect("complete");
        assert_eq!(kind, OperationKind::OpenMenu);
        assert!(!slot.is_busy());
    }

    #[test]
    fn second_begin_is_busy() {
        let mut slot = OperationSlot::new();
        let now = Instant::now();
        let _ = slot
            .begin(OperationKind::Launch, ident(1), Duration::from_secs(3), now)
            .unwrap();
        let err = slot
            .begin(OperationKind::Stop, ident(1), Duration::from_secs(40), now)
            .unwrap_err();
        assert_eq!(
            err,
            StartRejected::Busy {
                current: OperationKind::Launch
            }
        );
    }

    #[test]
    fn stale_token_is_rejected() {
        let mut slot = OperationSlot::new();
        let now = Instant::now();
        let id = ident(1);
        let token = slot
            .begin(OperationKind::Stop, id, Duration::from_secs(3), now)
            .unwrap();
        let _ = slot.abort_for_quit();
        let _ = slot
            .begin(OperationKind::Stop, id, Duration::from_secs(3), now)
            .unwrap();
        let err = slot.complete(token, id).unwrap_err();
        assert!(matches!(err, CompleteRejected::StaleToken { .. }));
    }

    #[test]
    fn identity_change_is_rejected() {
        let mut slot = OperationSlot::new();
        let now = Instant::now();
        let authorized = ident(1);
        let token = slot
            .begin(
                OperationKind::Stop,
                authorized,
                Duration::from_secs(40),
                now,
            )
            .unwrap();
        let observed = ident(2);
        let err = slot.complete(token, observed).unwrap_err();
        assert_eq!(err, CompleteRejected::IdentityChanged);
        assert!(slot.is_busy());
    }

    #[test]
    fn deadline_expiry_returns_kind_and_clears_slot() {
        let mut slot = OperationSlot::new();
        let t0 = Instant::now();
        let _ = slot
            .begin(OperationKind::Launch, ident(1), Duration::from_secs(40), t0)
            .unwrap();
        assert_eq!(slot.check_deadline(t0 + Duration::from_secs(39)), None);
        let kind = slot.check_deadline(t0 + Duration::from_secs(41));
        assert_eq!(kind, Some(OperationKind::Launch));
        assert!(!slot.is_busy());
    }

    #[test]
    fn shutdown_rejects_new_begin() {
        let mut slot = OperationSlot::new();
        slot.begin_shutdown();
        let err = slot
            .begin(
                OperationKind::OpenMenu,
                ident(1),
                Duration::from_secs(3),
                Instant::now(),
            )
            .unwrap_err();
        assert_eq!(err, StartRejected::ShuttingDown);
    }

    #[test]
    fn abort_for_quit_clears_slot_and_returns_kind() {
        let mut slot = OperationSlot::new();
        let _ = slot
            .begin(
                OperationKind::Stop,
                ident(1),
                Duration::from_secs(3),
                Instant::now(),
            )
            .unwrap();
        assert_eq!(slot.abort_for_quit(), Some(OperationKind::Stop));
        assert_eq!(slot.abort_for_quit(), None);
        assert!(!slot.is_busy());
    }
}
