use super::{item, MenuCommand, MenuItem};
use zaman_sessiond::display::{DisplayState, Mode, Output};

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayAction {
    Read,
    Preview { output: String, mode: Mode },
    Confirm(u64),
    Revert(u64),
}

#[derive(Debug, Default)]
pub struct DisplayControls {
    pub state: Option<DisplayState>,
    pub busy: bool,
    pub changing: bool,
    output: String,
    mode: Option<Mode>,
}
impl DisplayControls {
    pub fn output(&self) -> Option<&Output> {
        self.state.as_ref()?.outputs.iter().find(|o| o.name == self.output)
    }
    pub fn update(&mut self, state: DisplayState) -> Option<usize> {
        let before = self.state.as_ref().and_then(|s| s.preview.as_ref()).map(|p| p.token);
        let after = state.preview.as_ref().map(|p| p.token);
        self.state = Some(state);
        if self.output().is_none() {
            self.output = self.state.as_ref().unwrap().outputs.first()
                .map(|o| o.name.clone()).unwrap_or_default();
            self.mode = None;
        }
        if let Some(output) = self.output() {
            if before != after || self.mode.is_none_or(|m| !output.modes.contains(&m)) {
                self.mode = output.current_mode;
            }
        } else { self.mode = None; }
        if before != after { Some(if after.is_some() { 1 } else { 3 }) } else { None }
    }
    pub fn rows(&self) -> Vec<MenuItem> {
        if let Some(preview) = self.state.as_ref().and_then(|s| s.preview.as_ref()) {
            return vec![
                item("Keep changes", format!("Save {} for this display.", preview.mode.label()), MenuCommand::DisplayKeep),
                item("Revert", "Return to the previous display mode.", MenuCommand::DisplayRevert),
            ];
        }
        let Some(output) = self.output() else {
            return vec![
                item("Refresh displays", "Display settings are available in the Sway session.", MenuCommand::DisplayRead),
                item("Back", "Return to System.", MenuCommand::Back),
            ];
        };
        let mode = self.mode.or(output.current_mode);
        vec![
            item(format!("Display  {}", output.name), "Left / Right to select a connected display.", MenuCommand::DisplayOutput),
            item(mode.map(|m| format!("Resolution  {} x {}", m.width, m.height)).unwrap_or_else(|| "Resolution unavailable".into()),
                "Left / Right to choose an advertised resolution.", MenuCommand::DisplayResolution),
            item(mode.map(|m| format!("Refresh  {}.{:03} Hz", m.refresh / 1000, m.refresh % 1000)).unwrap_or_else(|| "Refresh unavailable".into()),
                "Left / Right to choose a supported refresh rate.", MenuCommand::DisplayRefresh),
            item("Apply", if mode == output.current_mode {
                "This mode is already active."
            } else { "Try this mode for 15 seconds, then Keep or Revert." }, MenuCommand::DisplayApply),
            item("Back", "Return to System.", MenuCommand::Back),
        ]
    }
    pub fn select(&mut self, command: MenuCommand) -> Option<DisplayAction> {
        if self.busy { return None }
        let preview = self.state.as_ref().and_then(|s| s.preview.as_ref());
        match command {
            MenuCommand::DisplayRead => Some(DisplayAction::Read),
            MenuCommand::DisplayApply if preview.is_none() => {
                let output = self.output()?;
                let mode = self.mode?;
                if output.current_mode == Some(mode) || !output.modes.contains(&mode) { return None }
                Some(DisplayAction::Preview { output: output.name.clone(), mode })
            }
            MenuCommand::DisplayKeep => preview.filter(|p| p.seconds_left > 0).map(|p| DisplayAction::Confirm(p.token)),
            MenuCommand::DisplayRevert | MenuCommand::Back => preview.map(|p| DisplayAction::Revert(p.token)),
            _ => None,
        }
    }
    pub fn cycle(&mut self, command: MenuCommand, forward: bool) {
        if self.busy || self.state.as_ref().is_some_and(|s| s.preview.is_some()) { return }
        if command == MenuCommand::DisplayOutput {
            let Some(state) = &self.state else { return };
            if state.outputs.is_empty() { return }
            let index = state.outputs.iter().position(|o| o.name == self.output).unwrap_or(0);
            let index = step(index, state.outputs.len(), forward);
            self.output = state.outputs[index].name.clone();
            self.mode = state.outputs[index].current_mode;
            return;
        }
        let Some(output) = self.output() else { return };
        let Some(mode) = self.mode else { return };
        let mut modes = output.modes.clone();
        modes.sort();
        modes.dedup();
        if command == MenuCommand::DisplayResolution {
            let mut sizes: Vec<_> = modes.iter().map(|m| (m.width, m.height)).collect();
            sizes.dedup();
            if sizes.is_empty() { return }
            let index = sizes.iter().position(|s| *s == (mode.width, mode.height)).unwrap_or(0);
            let size = sizes[step(index, sizes.len(), forward)];
            self.mode = modes.into_iter().filter(|m| (m.width, m.height) == size)
                .min_by_key(|m| m.refresh.abs_diff(mode.refresh));
        } else if command == MenuCommand::DisplayRefresh {
            modes.retain(|m| m.width == mode.width && m.height == mode.height);
            if modes.is_empty() { return }
            let index = modes.iter().position(|m| *m == mode).unwrap_or(0);
            self.mode = Some(modes[step(index, modes.len(), forward)]);
        }
    }
    pub fn detail(&self) -> String {
        if let Some(preview) = self.state.as_ref().and_then(|s| s.preview.as_ref()) {
            return format!("{}\n{}\nReverting in {} seconds.\nKeep saves this choice.",
                preview.output, preview.mode.label(), preview.seconds_left);
        }
        if let Some(output) = self.output() {
            let identity: String = format!("{} {}", output.make, output.model)
                .chars().filter(|c| !c.is_control()).collect();
            return format!("{}\nCurrent: {}", identity.trim(),
                output.current_mode.map(Mode::label).unwrap_or_else(|| "Unavailable".into()));
        }
        if self.state.as_ref().is_some_and(|s| !s.available) {
            "Display settings require the Sway session.".into()
        } else { "No active display available.".into() }
    }
}
fn step(index: usize, count: usize, forward: bool) -> usize {
    if forward { (index + 1) % count } else { (index + count - 1) % count }
}

/// Used exclusively by --preview and unit fixtures, with no compositor access.
pub fn preview_state(pending: bool) -> DisplayState {
    let mode = Mode { width: 2560, height: 1440, refresh: 59951 };
    DisplayState { available: true, outputs: vec![Output {
        id: 1, name: "HDMI-A-1".into(), make: "Acer Technologies".into(), model: "XB273U V3".into(),
        serial: "T45213B214205".into(), active: true, current_mode: Some(mode),
        modes: vec![mode, Mode { refresh: 119998, ..mode },
            Mode { width: 1920, height: 1080, refresh: 59940 },
            Mode { width: 1920, height: 1080, refresh: 120000 }],
    }], preview: pending.then_some(zaman_sessiond::display::Preview {
        token: 1, output: "HDMI-A-1".into(), mode, seconds_left: 12,
    }), error: String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolution_cycles_only_to_supported_pairs_and_retains_fractional_rates() {
        let mut controls = DisplayControls::default();
        controls.update(preview_state(false));
        controls.cycle(MenuCommand::DisplayResolution, true);
        let Some(DisplayAction::Preview { mode, .. }) = controls.select(MenuCommand::DisplayApply) else { panic!("mode not selectable") };
        assert_eq!(mode, Mode { width: 1920, height: 1080, refresh: 59940 });
        controls.cycle(MenuCommand::DisplayRefresh, true);
        let Some(DisplayAction::Preview { mode, .. }) = controls.select(MenuCommand::DisplayApply) else { panic!("mode not selectable") };
        assert_eq!(mode.refresh, 120000);
    }
    #[test]
    fn polling_preserves_unapplied_choice_but_revert_restores_actual_mode() {
        let mut controls = DisplayControls::default();
        controls.update(preview_state(false));
        controls.cycle(MenuCommand::DisplayResolution, true);
        controls.update(preview_state(false));
        assert!(controls.select(MenuCommand::DisplayApply).is_some());
        assert_eq!(controls.update(preview_state(true)), Some(1));
        assert_eq!(controls.select(MenuCommand::Back), Some(DisplayAction::Revert(1)));
        assert_eq!(controls.update(preview_state(false)), Some(3));
        assert!(controls.select(MenuCommand::DisplayApply).is_none());
    }
    #[test]
    fn disconnected_output_has_refresh_and_back_and_no_applicable_mode() {
        let mut controls = DisplayControls::default();
        let mut state = preview_state(false);
        state.outputs.clear();
        controls.update(state);
        assert_eq!(controls.rows().len(), 2);
        assert!(controls.select(MenuCommand::DisplayApply).is_none());
    }
}
