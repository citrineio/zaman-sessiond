#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideAction {
    Pressed,
    Released,
}

impl GuideAction {
    pub fn from_input_value(value: f64) -> Self {
        if value > 0.5 {
            Self::Pressed
        } else {
            Self::Released
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideEffect {
    None,
    OpenMenu,
    CloseOnReleaseArmed,
    CloseMenu,
}

#[derive(Debug, Default)]
pub struct GuideButton {
    pressed: bool,
    close_on_release: bool,
}

impl GuideButton {
    /// Opening remains press-triggered, but closing completes on release.
    /// InputPlumber must see that release while interception is still ALL;
    /// otherwise its D-Bus target can retain a pressed Guide state in PASS.
    pub fn input(&mut self, action: GuideAction, menu_open: bool) -> GuideEffect {
        match action {
            GuideAction::Pressed if self.pressed => GuideEffect::None,
            GuideAction::Pressed => {
                self.pressed = true;
                self.close_on_release = menu_open;
                if menu_open {
                    GuideEffect::CloseOnReleaseArmed
                } else {
                    GuideEffect::OpenMenu
                }
            }
            GuideAction::Released if !self.pressed => GuideEffect::None,
            GuideAction::Released => {
                self.pressed = false;
                if std::mem::take(&mut self.close_on_release) {
                    GuideEffect::CloseMenu
                } else {
                    GuideEffect::None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GuideAction, GuideButton, GuideEffect};

    #[test]
    fn normalized_values_become_button_edges() {
        assert_eq!(GuideAction::from_input_value(1.0), GuideAction::Pressed);
        assert_eq!(GuideAction::from_input_value(0.0), GuideAction::Released);
    }

    #[test]
    fn opening_happens_on_press_and_does_not_close_on_the_same_release() {
        let mut button = GuideButton::default();
        assert_eq!(
            button.input(GuideAction::Pressed, false),
            GuideEffect::OpenMenu
        );
        assert_eq!(button.input(GuideAction::Pressed, true), GuideEffect::None);
        assert_eq!(button.input(GuideAction::Released, true), GuideEffect::None);
    }

    #[test]
    fn closing_waits_for_the_release_before_restoring_pass_through() {
        let mut button = GuideButton::default();
        assert_eq!(
            button.input(GuideAction::Pressed, true),
            GuideEffect::CloseOnReleaseArmed
        );
        assert_eq!(
            button.input(GuideAction::Released, true),
            GuideEffect::CloseMenu
        );
    }

    #[test]
    fn stale_release_is_ignored() {
        let mut button = GuideButton::default();
        assert_eq!(
            button.input(GuideAction::Released, false),
            GuideEffect::None
        );
    }
}


// A single Guide cycle spans duplicate USB/Bluetooth targets. Lifecycle resets
// clear the state without issuing a close. Closing remains release-triggered.
#[derive(Debug, Default)]
pub struct GuideRouter {
    pressed: std::collections::BTreeSet<String>,
    close: Option<(String, GuideClose)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuideClose {
    pub session: String,
    pub identity: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutedGuide { None, Open, CloseArmed }

impl GuideRouter {
    pub fn clear(&mut self) { self.pressed.clear(); self.close = None; }

    pub fn needs_status(&self, target: &str) -> bool {
        !self.pressed.contains(target) && self.pressed.is_empty()
    }

    pub fn suppress_press(&mut self, target: &str) {
        self.pressed.insert(target.to_owned());
    }

    pub fn press(&mut self, target: &str, status: &str, session: &str, identity: &str) -> RoutedGuide {
        let first = self.pressed.is_empty();
        if !self.pressed.insert(target.to_owned()) || !first { return RoutedGuide::None; }
        match status {
            "hidden" => RoutedGuide::Open,
            "open" => {
                self.close = Some((target.to_owned(), GuideClose { session: session.to_owned(), identity: identity.to_owned() }));
                RoutedGuide::CloseArmed
            }
            _ => RoutedGuide::None,
        }
    }

    pub fn release(&mut self, target: &str) -> Option<GuideClose> {
        if !self.pressed.remove(target) { return None; }
        if self.close.as_ref().is_some_and(|(owner, _)| owner == target) {
            return self.close.take().map(|(_, session)| session);
        }
        None
    }
}

#[cfg(test)]
mod router_tests {
    use super::*;

    #[test]
    fn opening_release_never_closes_and_next_cycle_can_close() {
        let mut route = GuideRouter::default();
        assert_eq!(route.press("usb", "hidden", "i:0", "live-source"), RoutedGuide::Open);
        assert_eq!(route.release("usb"), None);
        assert_eq!(route.press("usb", "open", "i:1", "live-source"), RoutedGuide::CloseArmed);
        assert_eq!(route.release("usb"), Some(GuideClose { session: "i:1".into(), identity: "live-source".into() }));
    }

    #[test]
    fn duplicate_transport_and_repeat_cannot_toggle_twice() {
        let mut route = GuideRouter::default();
        assert_eq!(route.press("usb", "hidden", "i:0", "live-source"), RoutedGuide::Open);
        assert!(!route.needs_status("usb"));
        route.suppress_press("usb");
        assert!(!route.needs_status("bt"));
        route.suppress_press("bt");
        assert_eq!(route.release("usb"), None);
        assert_eq!(route.release("bt"), None);
        assert!(route.needs_status("usb"));
    }

    #[test]
    fn only_arming_source_release_can_close_its_session() {
        let mut route = GuideRouter::default();
        route.press("usb", "open", "i:7", "live-source");
        route.suppress_press("bt");
        assert_eq!(route.release("bt"), None);
        assert_eq!(route.release("usb"), Some(GuideClose { session: "i:7".into(), identity: "live-source".into() }));
        assert_eq!(route.release("usb"), None);
    }

    #[test]
    fn controller_or_owner_loss_cancels_close_without_closing() {
        let mut route = GuideRouter::default();
        route.press("usb", "open", "i:9", "live-source");
        route.clear();
        assert_eq!(route.release("usb"), None);
        assert!(route.needs_status("bt"));
    }

    #[test]
    fn transition_and_busy_press_cannot_open_or_arm_close() {
        for state in ["opening", "closing", "busy"] {
            let mut route = GuideRouter::default();
            assert_eq!(route.press("usb", state, "i:1", "live-source"), RoutedGuide::None);
            assert_eq!(route.release("usb"), None);
        }
    }
}
