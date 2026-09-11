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
