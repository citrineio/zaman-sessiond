//! Display preferences and timed previews. The renderer only consumes snapshots.
//! All IPC and persistence run on blocking workers, never the D-Bus executor.
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, String>;
const PREVIEW_TIME: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Millihertz, as reported by Sway. Never round this to an integer Hz.
    pub refresh: u32,
}
impl Mode {
    pub fn label(self) -> String {
        format!("{} x {} @ {}.{:03} Hz", self.width, self.height,
            self.refresh / 1000, self.refresh % 1000)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Output {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub make: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub serial: String,
    pub active: bool,
    pub current_mode: Option<Mode>,
    #[serde(default)]
    pub modes: Vec<Mode>,
}
impl Output {
    fn same_display(&self, other: &Self) -> bool {
        self.name == other.name && self.make == other.make
            && self.model == other.model && self.serial == other.serial
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Preview {
    pub token: u64,
    pub output: String,
    pub mode: Mode,
    pub seconds_left: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DisplayState {
    pub available: bool,
    pub outputs: Vec<Output>,
    pub preview: Option<Preview>,
    pub error: String,
}

trait Backend: Send + Sync {
    fn outputs(&self) -> Result<Vec<Output>>;
    fn set_mode(&self, output: &str, mode: Mode) -> Result<()>;
    fn set_preferred(&self, output: &str) -> Result<Mode>;

}

struct Sway { socket: PathBuf }
impl Sway {
    fn request(&self, kind: u32, payload: &[u8]) -> Result<Vec<u8>> {
        let operation = || -> std::io::Result<Vec<u8>> {
            let mut socket = UnixStream::connect(&self.socket)?;
            socket.set_read_timeout(Some(Duration::from_secs(2)))?;
            socket.set_write_timeout(Some(Duration::from_secs(2)))?;
            socket.write_all(b"i3-ipc")?;
            socket.write_all(&(payload.len() as u32).to_ne_bytes())?;
            socket.write_all(&kind.to_ne_bytes())?;
            socket.write_all(payload)?;
            let mut header = [0u8; 14];
            socket.read_exact(&mut header)?;
            let size = u32::from_ne_bytes(header[6..10].try_into().unwrap()) as usize;
            let response_kind = u32::from_ne_bytes(header[10..14].try_into().unwrap());
            if &header[..6] != b"i3-ipc" || response_kind != kind || size > 4 * 1024 * 1024 {
                return Err(std::io::Error::other("invalid Sway IPC response"));
            }
            let mut response = vec![0; size];
            socket.read_exact(&mut response)?;
            Ok(response)
        };
        operation().map_err(|e| format!("Sway IPC: {e}"))
    }
}
impl Backend for Sway {
    fn outputs(&self) -> Result<Vec<Output>> {
        let outputs: Vec<Output> = serde_json::from_slice(&self.request(3, b"")?)
            .map_err(|e| format!("Sway outputs: {e}"))?;
        Ok(outputs.into_iter().filter(|o| o.active && o.current_mode.is_some()).collect())
    }
    fn set_mode(&self, output: &str, mode: Mode) -> Result<()> {
        self.command(output, &format!("{}x{}@{}.{:03}Hz",
            mode.width, mode.height, mode.refresh / 1000, mode.refresh % 1000))
    }
    fn set_preferred(&self, output: &str) -> Result<Mode> {
        // Sway 1.10 output_cmd_mode accepts 0x0; queue_output_config treats
        // nonpositive dimensions as automatic and asks wlroots for preferred.
        // This clears a previous runtime mode without guessing EDID order.
        // Never use --custom here. Verify an advertised mode was selected.
        self.command(output, "0x0")?;
        let current = self.outputs()?.into_iter().find(|o| o.name == output)
            .ok_or_else(|| "Output disconnected during preferred-mode selection".to_string())?;
        current.current_mode.filter(|m| current.modes.contains(m))
            .ok_or_else(|| "Sway did not select an advertised preferred mode".into())
    }
}
impl Sway {
    fn command(&self, output: &str, mode: &str) -> Result<()> {
        // Only connector identifiers returned by GET_OUTPUTS are accepted. Never
        // interpolate monitor descriptions or user-provided command fragments.
        if output.is_empty() || !output.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) {
            return Err("invalid output connector".into());
        }
        let command = format!("output {output} mode {mode}");
        #[derive(Deserialize)]
        struct Reply { success: bool, #[serde(default)] error: String }
        let replies: Vec<Reply> = serde_json::from_slice(&self.request(0, command.as_bytes())?)
            .map_err(|e| e.to_string())?;
        if replies.len() != 1 || !replies[0].success {
            return Err(replies.first().map(|r| r.error.clone())
                .filter(|e| !e.is_empty()).unwrap_or_else(|| "Sway rejected the mode".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Preference { output: Output, mode: Mode }
struct Pending {
    token: u64,
    generation: u64,
    output: Output,
    previous: Mode,
    desired: Mode,
    deadline: Instant,
}
struct Controller {
    backend: Option<Box<dyn Backend>>,
    path: Option<PathBuf>,
    preferences: Vec<Preference>,
    pending: Option<Pending>,
    token: u64,
    error: String,
    valid_menu: Arc<dyn Fn(u64) -> bool + Send + Sync>,
    auto_seen: Vec<Output>,
    next_auto_check: Instant,
}

#[derive(Clone)]
pub struct DisplayManager { inner: Arc<Mutex<Controller>> }
impl DisplayManager {
    /// Called from a blocking worker during daemon startup. Existing Cage
    /// sessions remain supported; absence of SWAYSOCK disables this page only.
    pub fn new(valid_menu: impl Fn(u64) -> bool + Send + Sync + 'static) -> Self {
        let backend = std::env::var_os("SWAYSOCK")
            .map(|socket| Box::new(Sway { socket: socket.into() }) as Box<dyn Backend>);
        let path = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("zaman/display.json"));
        let mut state = Controller {
            backend, path, preferences: vec![], pending: None,
            token: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64,
            error: String::new(), valid_menu: Arc::new(valid_menu),
            auto_seen: vec![], next_auto_check: Instant::now(),
        };
        if let Err(error) = state.restore_saved() { state.error = error; }
        if let Err(error) = state.apply_automatic() { state.error = error; }
        let manager = Self { inner: Arc::new(Mutex::new(state)) };
        let weak = Arc::downgrade(&manager.inner);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let Some(inner) = weak.upgrade() else { break };
            let mut state = inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(error) = state.expire() {
                if state.error != error { eprintln!("Display rollback: {error}"); }
                state.error = error;
            }
            if Instant::now() >= state.next_auto_check {
                state.next_auto_check = Instant::now() + Duration::from_secs(2);
                if let Err(error) = state.apply_automatic() {
                    if state.error != error { eprintln!("Display automatic mode: {error}"); }
                    state.error = error;
                }
            }
        });
        manager
    }
    fn with<T>(&self, operation: impl FnOnce(&mut Controller) -> Result<T>) -> Result<T> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let result = operation(&mut state);
        if let Err(error) = &result { state.error.clone_from(error); }
        result
    }
    pub fn status(&self) -> Result<DisplayState> { self.with(|s| s.status()) }
    pub fn preview(&self, generation: u64, output: String, mode: Mode) -> Result<DisplayState> {
        self.with(|s| { s.preview(generation, &output, mode)?; s.status() })
    }
    pub fn confirm(&self, generation: u64, token: u64) -> Result<DisplayState> {
        self.with(|s| { s.confirm(generation, token)?; s.status() })
    }
    pub fn revert(&self, generation: u64, token: u64) -> Result<DisplayState> {
        self.with(|s| { s.require_token(generation, token)?; s.rollback()?; s.status() })
    }
    pub fn shutdown(&self) -> Result<()> {
        self.with(|s| if s.pending.is_some() { s.rollback() } else { Ok(()) })
    }
}
impl Controller {
    fn expire(&mut self) -> Result<()> {
        if self.pending.as_ref().is_some_and(|p|
            Instant::now() >= p.deadline || !(self.valid_menu)(p.generation)) {
            self.rollback()?;
        }
        Ok(())
    }
    fn backend(&self) -> Result<&dyn Backend> {
        self.backend.as_deref().ok_or_else(|| "Display settings require the Sway session".into())
    }
    fn status(&mut self) -> Result<DisplayState> {
        if self.backend.is_none() {
            return Ok(DisplayState { available: false, outputs: vec![], preview: None,
                error: "Display settings require the Sway session".into() });
        }
        let outputs = self.backend()?.outputs()?;
        Ok(DisplayState { available: true, outputs,
            preview: self.pending.as_ref().map(|p| Preview {
                token: p.token, output: p.output.name.clone(), mode: p.desired,
                seconds_left: p.deadline.saturating_duration_since(Instant::now()).as_millis().div_ceil(1000) as u64,
            }), error: self.error.clone() })
    }
    fn preview(&mut self, generation: u64, name: &str, desired: Mode) -> Result<()> {
        if self.pending.is_some() { return Err("Keep or revert the current preview first".into()); }
        if !(self.valid_menu)(generation) { return Err("stale or closed menu".into()); }
        if self.path.is_none() { return Err("Display preferences have no config directory".into()); }
        let output = self.backend()?.outputs()?.into_iter().find(|o| o.name == name)
            .ok_or_else(|| "Output is no longer connected".to_string())?;
        if !output.modes.contains(&desired) { return Err("Mode is no longer advertised by this output".into()); }
        let previous = output.current_mode.ok_or_else(|| "Output has no current mode".to_string())?;
        if previous == desired { return Err("This mode is already active".into()); }
        // Record rollback BEFORE sending the command: an IPC timeout can happen
        // after Sway applies it. Only readback determines whether it took effect.
        self.token = self.token.wrapping_add(1).max(1);
        self.pending = Some(Pending { token: self.token, generation, output: output.clone(),
            previous, desired, deadline: Instant::now() + PREVIEW_TIME });
        let result = self.backend()?.set_mode(name, desired)
            .and_then(|_| self.verify(&output, desired));
        if let Err(error) = result {
            // A failed/uncertain apply must never become confirmable, even if
            // its first recovery attempt also encounters an IPC failure.
            if let Some(pending) = self.pending.as_mut() { pending.deadline = Instant::now(); }
            let rollback = self.rollback();
            return Err(match rollback {
                Ok(()) => error,
                Err(recovery) => format!("{error}; rollback: {recovery}"),
            });
        }
        self.pending.as_mut().unwrap().deadline = Instant::now() + PREVIEW_TIME;
        self.error.clear();
        Ok(())
    }
    fn verify(&self, output: &Output, mode: Mode) -> Result<()> {
        let outputs = self.backend()?.outputs()?;
        if outputs.iter().any(|o| o.same_display(output) && o.current_mode == Some(mode)) {
            Ok(())
        } else { Err("The output did not reach the requested mode".into()) }
    }
    fn require_token(&self, generation: u64, token: u64) -> Result<()> {
        match &self.pending {
            Some(p) if p.generation == generation && p.token == token => Ok(()),
            _ => Err("Display preview is no longer active".into()),
        }
    }
    fn confirm(&mut self, generation: u64, token: u64) -> Result<()> {
        self.require_token(generation, token)?;
        let pending = self.pending.as_ref().unwrap();
        if Instant::now() >= pending.deadline || !(self.valid_menu)(generation) {
            self.rollback()?;
            return Err("Display preview expired or the menu closed".into());
        }
        self.verify(&pending.output, pending.desired)?;
        if Instant::now() >= pending.deadline || !(self.valid_menu)(generation) {
            self.rollback()?;
            return Err("Display preview expired or the menu closed".into());
        }
        let pending = self.pending.as_ref().unwrap();
        let preference = Preference { output: pending.output.clone(), mode: pending.desired };
        let mut preferences = self.preferences.clone();
        preferences.retain(|p| !p.output.same_display(&preference.output));
        preferences.push(preference);
        let path = self.path.as_ref().ok_or_else(|| "No preference path".to_string())?;
        write_preferences(path, &preferences, token)?;
        // Confirmation is accepted when its verified request reaches this
        // transaction before the deadline. The mutex serializes it with rollback.
        self.preferences = preferences;
        self.pending = None;
        self.error.clear();
        Ok(())
    }
    fn rollback(&mut self) -> Result<()> {
        let Some(pending) = self.pending.as_ref() else { return Ok(()) };
        let outputs = self.backend()?.outputs()?;
        let Some(output) = outputs.iter().find(|o| o.same_display(&pending.output)) else {
            // Never apply the old mode to a different monitor on this connector.
            self.pending = None;
            return Err("Preview cancelled because the display disconnected or changed".into());
        };
        if output.current_mode != Some(pending.previous) {
            self.backend()?.set_mode(&output.name, pending.previous)?;
            self.verify(output, pending.previous)?;
        }
        self.pending = None;
        self.error.clear();
        Ok(())
    }
    fn restore_saved(&mut self) -> Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        match fs::read(path) {
            Ok(bytes) => self.preferences = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Display preferences: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(format!("Display preferences: {e}")),
        }
        // Retain manual preferences for compatibility/rollback, but the new
        // automatic policy owns startup/reconnection instead of old choices.
        Ok(())
    }
    fn apply_automatic(&mut self) -> Result<()> {
        if self.backend.is_none() || self.pending.is_some() { return Ok(()) }
        let outputs = self.backend()?.outputs()?;
        self.auto_seen.retain(|old| outputs.iter().any(|o| o.same_display(old)));
        let mut errors = vec![];
        for output in outputs {
            if output.modes.is_empty() { continue }
            let mut signature = output.clone();
            signature.modes.sort();
            signature.modes.dedup();
            if self.auto_seen.iter().any(|old|
                old.same_display(&signature) && old.id == signature.id && old.modes == signature.modes) { continue }
            let result = self.select_automatic(&output);
            // One attempt per capability snapshot, including failures. Avoid
            // repeatedly flashing a rejected mode on an otherwise stable TV.
            self.auto_seen.retain(|old| !old.same_display(&signature));
            self.auto_seen.push(signature);
            if let Err(error) = result { errors.push(error); }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
    fn select_automatic(&self, output: &Output) -> Result<()> {
        let previous = output.current_mode;
        let mut errors = vec![];
        for mode in automatic_candidates(&output.modes) {
            let current = self.connected_display(output)?;
            if !current.modes.contains(&mode) { continue }
            if current.current_mode == Some(mode) { return Ok(()) }
            match self.backend()?.set_mode(&output.name, mode)
                .and_then(|_| self.verify(output, mode)) {
                Ok(()) => {
                    eprintln!("Display automatic: {} {}", output.name, mode.label());
                    return Ok(())
                }
                Err(error) => errors.push(error),
            }
        }
        self.connected_display(output)?;
        let preferred = self.backend()?.set_preferred(&output.name)
            .and_then(|mode| self.verify(output, mode).map(|_| mode));
        match preferred {
            Ok(mode) => {
                eprintln!("Display automatic: {} preferred {}", output.name, mode.label());
                Ok(())
            }
            Err(error) => {
                errors.push(error);
                // If all automatic candidates were rejected, restore the exact
                // starting mode only while it is still the same display.
                if let Ok(current) = self.connected_display(output) {
                    if let Some(mode) = previous.filter(|m| current.modes.contains(m)) {
                        if let Err(error) = self.backend()?.set_mode(&output.name, mode)
                            .and_then(|_| self.verify(output, mode)) {
                            errors.push(format!("restore: {error}"));
                        }
                    }
                }
                Err(format!("{} automatic selection failed: {}", output.name, errors.join("; ")))
            }
        }
    }
    fn connected_display(&self, output: &Output) -> Result<Output> {
        self.backend()?.outputs()?.into_iter().find(|o| o.same_display(output))
            .ok_or_else(|| "Display disconnected or changed during automatic selection".into())
    }
}

fn automatic_candidates(modes: &[Mode]) -> Vec<Mode> {
    let mut candidates = vec![];
    for (width, height) in [(1920, 1080), (1280, 720)] {
        let mut tier: Vec<_> = modes.iter().copied().filter(|m|
            m.width == width && m.height == height && m.refresh.abs_diff(60_000) <= 100).collect();
        // Exact 60.000 wins, otherwise use the closest advertised 60-family
        // timing (including 59.940). Never round or synthesize a rate.
        tier.sort_by_key(|m| (m.refresh.abs_diff(60_000), m.refresh));
        tier.dedup();
        candidates.extend(tier);
    }
    candidates

}
fn write_preferences(path: &PathBuf, preferences: &[Preference], token: u64) -> Result<()> {
    let parent = path.parent().ok_or_else(|| "Invalid preference path".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".display-{}-{token}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        let bytes = serde_json::to_vec_pretty(preferences).map_err(std::io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result.map_err(|e| format!("Could not save display preference: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    const ORIGINAL: Mode = Mode { width: 2560, height: 1440, refresh: 59951 };
    const TEST_MODE: Mode = Mode { width: 1920, height: 1080, refresh: 59940 };
    static NEXT: AtomicU64 = AtomicU64::new(1);
    #[derive(Default)]
    struct Fake {
        outputs: Mutex<Vec<Output>>,
        commands: Mutex<Vec<(String, Mode)>>,
        ignore_next: AtomicBool,
        preferred: Mutex<Option<Mode>>,
        preferred_calls: AtomicU64,
        reject: Mutex<Vec<Mode>>,
    }
    impl Backend for Arc<Fake> {
        fn outputs(&self) -> Result<Vec<Output>> { Ok(self.outputs.lock().unwrap().clone()) }
        fn set_mode(&self, name: &str, mode: Mode) -> Result<()> {
            self.commands.lock().unwrap().push((name.into(), mode));
            if self.reject.lock().unwrap().contains(&mode) { return Err("rejected".into()) }
            if !self.ignore_next.swap(false, Ordering::SeqCst) {
                let mut outputs = self.outputs.lock().unwrap();
                let output = outputs.iter_mut().find(|o| o.name == name)
                    .ok_or_else(|| "disconnected".to_string())?;
                output.current_mode = Some(mode);
            }
            Ok(())
        }
        fn set_preferred(&self, name: &str) -> Result<Mode> {
            self.preferred_calls.fetch_add(1, Ordering::SeqCst);
            let mode = self.preferred.lock().unwrap().ok_or_else(|| "no preferred mode".to_string())?;
            self.set_mode(name, mode)?;
            Ok(mode)
        }
    }
    fn fixture() -> (Controller, Arc<Fake>, PathBuf) {
        let fake = Arc::new(Fake::default());
        fake.outputs.lock().unwrap().push(Output { id: 1, name: "HDMI-A-1".into(),
            make: "Acer".into(), model: "XB273U V3".into(), serial: "serial-A".into(),
            active: true, current_mode: Some(ORIGINAL), modes: vec![ORIGINAL, TEST_MODE] });
        let directory = std::env::temp_dir().join(format!("zaman-display-{}-{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let state = Controller { backend: Some(Box::new(fake.clone())),
            path: Some(directory.join("display.json")), preferences: vec![], pending: None,
            token: 100, error: String::new(), valid_menu: Arc::new(|g| g == 7),
            auto_seen: vec![], next_auto_check: Instant::now() };
        (state, fake, directory)
    }
    #[test]
    fn timeout_restores_exact_fractional_mode_without_saving() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        state.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
        state.expire().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        assert!(state.pending.is_none());
        assert!(!directory.exists());
    }
    #[test]
    fn closing_menu_rolls_back_before_timeout() {
        let (mut state, fake, _) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        state.valid_menu = Arc::new(|_| false);
        state.expire().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
    }
    #[test]
    fn stale_tokens_and_unsupported_modes_cannot_change_or_save() {
        let (mut state, fake, directory) = fixture();
        assert!(state.preview(6, "HDMI-A-1", TEST_MODE).is_err());
        assert!(state.preview(7, "HDMI-A-1", Mode { refresh: 60000, ..TEST_MODE }).is_err());
        assert!(fake.commands.lock().unwrap().is_empty());
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let token = state.pending.as_ref().unwrap().token;
        assert!(state.confirm(6, token).is_err());
        assert!(state.confirm(7, token - 1).is_err());
        assert!(!directory.exists());
        state.rollback().unwrap();
    }
    #[test]
    fn success_reply_is_insufficient_without_matching_readback() {
        let (mut state, fake, _) = fixture();
        fake.ignore_next.store(true, Ordering::SeqCst);
        assert!(state.preview(7, "HDMI-A-1", TEST_MODE).is_err());
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        assert!(state.pending.is_none());
    }
    #[test]
    fn keep_persists_but_old_choices_do_not_override_automatic_policy() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let token = state.pending.as_ref().unwrap().token;
        state.confirm(7, token).unwrap();
        assert!(state.pending.is_none());
        let saved: Vec<Preference> = serde_json::from_slice(&fs::read(state.path.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(saved[0].mode, TEST_MODE);
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        state.restore_saved().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(TEST_MODE));
        fake.outputs.lock().unwrap()[0].serial = "another-display".into();
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        state.restore_saved().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn disconnected_or_replaced_display_is_not_sent_the_old_mode() {
        let (mut state, fake, _) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        fake.outputs.lock().unwrap()[0].serial = "replacement".into();
        state.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
        assert!(state.expire().is_err());
        assert!(state.pending.is_none());
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
    }
    #[test]
    fn persistence_failure_leaves_preview_available_for_revert() {
        let (mut state, fake, directory) = fixture();
        // Using a directory as the destination makes the atomic rename fail.
        fs::create_dir_all(state.path.as_ref().unwrap()).unwrap();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let token = state.pending.as_ref().unwrap().token;
        assert!(state.confirm(7, token).is_err());
        assert!(state.pending.is_some());
        state.rollback().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        fs::remove_dir_all(directory).unwrap();
    }
    const HD: Mode = Mode { width: 1280, height: 720, refresh: 60_000 };
    const FHD: Mode = Mode { width: 1920, height: 1080, refresh: 60_000 };
    const UHD30: Mode = Mode { width: 3840, height: 2160, refresh: 30_000 };
    #[test]
    fn auto_orders_resolution_then_exact_or_fractional_sixty() {
        assert_eq!(automatic_candidates(&[UHD30, HD, TEST_MODE, FHD, ORIGINAL]),
            vec![FHD, TEST_MODE, HD]);
        assert!(automatic_candidates(&[UHD30, Mode { refresh: 50_000, ..FHD },
            Mode { refresh: 120_000, ..HD }]).is_empty());
    }
    #[test]
    fn auto_selects_1080_before_720_and_does_not_loop_on_its_own_changes() {
        let (mut state, fake, _) = fixture();
        fake.outputs.lock().unwrap()[0].modes = vec![HD, TEST_MODE, FHD];
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(FHD));
        state.apply_automatic().unwrap();
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
        assert_eq!(fake.preferred_calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn auto_falls_back_to_720_when_1080_is_missing_or_rejected() {
        let (mut state, fake, _) = fixture();
        fake.outputs.lock().unwrap()[0].modes = vec![HD, ORIGINAL];
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(HD));
        fake.outputs.lock().unwrap()[0].modes.push(FHD);
        fake.reject.lock().unwrap().push(FHD);
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(HD));
    }
    #[test]
    fn auto_asks_backend_for_preferred_not_first_or_largest_mode() {
        let (mut state, fake, _) = fixture();
        fake.outputs.lock().unwrap()[0].modes = vec![UHD30, ORIGINAL];
        *fake.preferred.lock().unwrap() = Some(ORIGINAL);
        state.apply_automatic().unwrap();
        assert_eq!(fake.preferred_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
    }
    #[test]
    fn auto_defers_during_preview_and_leaves_manual_changes_until_reconnect() {
        let (mut state, fake, _) = fixture();
        state.apply_automatic().unwrap();
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        // An ordinary manual/current-mode change must not fight the user.
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        fake.outputs.lock().unwrap()[0].modes.push(HD);
        let count = fake.commands.lock().unwrap().len();
        state.apply_automatic().unwrap();
        assert_eq!(fake.commands.lock().unwrap().len(), count);
        state.rollback().unwrap();
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(TEST_MODE));
        let output = fake.outputs.lock().unwrap().remove(0);
        state.apply_automatic().unwrap();
        fake.outputs.lock().unwrap().push(Output { current_mode: Some(ORIGINAL), ..output });
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(TEST_MODE));
    }
    #[test]
    fn auto_failed_modes_restore_previous_and_are_not_retried_continuously() {
        let (mut state, fake, _) = fixture();
        fake.reject.lock().unwrap().push(TEST_MODE);
        state.apply_automatic().unwrap_err();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        let count = fake.commands.lock().unwrap().len();
        state.apply_automatic().unwrap();
        assert_eq!(fake.commands.lock().unwrap().len(), count);
    }

    #[test]
    fn auto_detects_fast_reconnect_by_new_sway_output_id() {
        let (mut state, fake, _) = fixture();
        state.apply_automatic().unwrap();
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        fake.outputs.lock().unwrap()[0].id = 2;
        state.apply_automatic().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(TEST_MODE));
    }

}
