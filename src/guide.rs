#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideAction {
    MenuRequested,
    Released,
}

#[derive(Debug, Default)]
pub struct GuideButton {
    pressed: bool,
}

impl GuideButton {
    pub fn press(&mut self) -> Vec<GuideAction> {
        if self.pressed {
            return Vec::new();
        }

        self.pressed = true;
        vec![GuideAction::MenuRequested]
    }

    pub fn release(&mut self) -> Vec<GuideAction> {
        if !self.pressed {
            return Vec::new();
        }

        self.pressed = false;
        vec![GuideAction::Released]
    }
}

#[cfg(test)]
mod tests {
    use super::{GuideAction, GuideButton};

    #[test]
    fn press_requests_the_menu_immediately() {
        let mut button = GuideButton::default();
        assert_eq!(button.press(), vec![GuideAction::MenuRequested]);
    }

    #[test]
    fn duplicate_press_is_ignored_until_release() {
        let mut button = GuideButton::default();
        assert_eq!(button.press(), vec![GuideAction::MenuRequested]);
        assert!(button.press().is_empty());
    }

    #[test]
    fn release_rearms_the_button_without_requesting_another_menu() {
        let mut button = GuideButton::default();
        button.press();
        assert_eq!(button.release(), vec![GuideAction::Released]);
        assert_eq!(button.press(), vec![GuideAction::MenuRequested]);
    }

    #[test]
    fn release_without_a_press_is_ignored() {
        let mut button = GuideButton::default();
        assert!(button.release().is_empty());
    }
}
