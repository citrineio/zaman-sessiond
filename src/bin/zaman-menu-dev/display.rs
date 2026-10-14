//! Development-only display transactions, adapted from the supplied daemon.
//! Construction reads preferences but NEVER applies them. Keep is the sole save.
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Output {
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
    #[serde(serialize_with = "serialize_token")]
    pub token: u64,
    pub output: String,
    pub mode: Mode,
    pub seconds_left: u64,
    pub confirmable: bool,
}
fn serialize_token<S: serde::Serializer>(token: &u64, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&token.to_string())
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
        // Only connector identifiers returned by GET_OUTPUTS are accepted. Never
        // interpolate monitor descriptions or user-provided command fragments.
        if output.is_empty() || !output.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) {
            return Err("invalid output connector".into());
        }
        let command = format!("output {output} mode {}x{}@{}.{:03}Hz",
            mode.width, mode.height, mode.refresh / 1000, mode.refresh % 1000);
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
    confirmable: bool,
}
struct Controller {
    backend: Option<Box<dyn Backend>>,
    path: Option<PathBuf>,
    preferences: Vec<Preference>,
    preferences_ready: bool,
    pending: Option<Pending>,
    token: u64,
    error: String,
    valid_menu: Arc<dyn Fn(u64) -> bool + Send + Sync>,
}

#[derive(Clone)]
pub struct DisplayManager { inner: Arc<Mutex<Controller>> }
impl DisplayManager {
    /// Called on a blocking worker; opening the page never restores saved modes.
    pub fn new(valid_menu: impl Fn(u64) -> bool + Send + Sync + 'static) -> Self {
        let backend = std::env::var_os("SWAYSOCK")
            .map(|socket| Box::new(Sway { socket: socket.into() }) as Box<dyn Backend>);
        let path = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("zaman/display.json"));
        let mut state = Controller {
            backend, path, preferences: vec![], preferences_ready: false, pending: None,
            token: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64,
            error: String::new(), valid_menu: Arc::new(valid_menu),
        };
        if let Err(error) = state.load_saved() { state.error = error; }
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
    pub fn pending(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).pending.is_some()
    }
    pub fn preview_checked(&self, generation: u64, output: Output, mode: Mode) -> Result<DisplayState> {
        self.with(|s| { s.preview_for(generation, &output.name, mode, Some(&output))?; s.status() })
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
        self.expire()?;
        if self.backend.is_none() {
            return Ok(DisplayState { available: false, outputs: vec![], preview: None,
                error: "Display settings require the Sway session".into() });
        }
        let outputs = self.backend()?.outputs()?;
        Ok(DisplayState { available: true, outputs,
            preview: self.pending.as_ref().map(|p| Preview {
                token: p.token, output: p.output.name.clone(), mode: p.desired,
                seconds_left: p.deadline.saturating_duration_since(Instant::now()).as_millis().div_ceil(1000) as u64,
                confirmable: p.confirmable && (self.valid_menu)(p.generation) && Instant::now() < p.deadline,
            }), error: self.error.clone() })
    }
    #[cfg(test)]
    fn preview(&mut self, generation: u64, name: &str, desired: Mode) -> Result<()> {
        self.preview_for(generation, name, desired, None)
    }
    fn preview_for(&mut self, generation: u64, name: &str, desired: Mode, expected: Option<&Output>) -> Result<()> {
        if self.pending.is_some() { return Err("Keep or revert the current preview first".into()); }
        if !(self.valid_menu)(generation) { return Err("stale or closed menu".into()); }
        if self.path.is_none() { return Err("Display preferences have no config directory".into()); }
        if !self.preferences_ready { self.load_saved()?; }
        let output = self.backend()?.outputs()?.into_iter().find(|o| o.name == name)
            .ok_or_else(|| "Output is no longer connected".to_string())?;
        if expected.is_some_and(|old| !old.same_display(&output)) {
            return Err("Selected display changed; refresh before previewing".into());
        }
        if !output.modes.contains(&desired) { return Err("Mode is no longer advertised by this output".into()); }
        let previous = output.current_mode.ok_or_else(|| "Output has no current mode".to_string())?;
        if !output.modes.contains(&previous) {
            return Err("The current mode is not advertised; a safe rollback cannot be prepared".into());
        }
        if previous == desired { return Err("This mode is already active".into()); }
        if !(self.valid_menu)(generation) { return Err("Display page closed before apply".into()); }
        // Record rollback BEFORE sending the command: an IPC timeout can happen
        // after Sway applies it. Only readback determines whether it took effect.
        self.token = self.token.wrapping_add(1).max(1);
        self.pending = Some(Pending { token: self.token, generation, output: output.clone(),
            previous, desired, deadline: Instant::now() + PREVIEW_TIME, confirmable: false });
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
        self.pending.as_mut().unwrap().confirmable = true;
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
        // Merge with the latest preferences, not a stale startup snapshot.
        // A malformed/unreadable file must never be replaced by an empty set.
        self.load_saved()?;
        let pending = self.pending.as_ref().unwrap();
        if !pending.confirmable || Instant::now() >= pending.deadline || !(self.valid_menu)(generation) {
            self.rollback()?;
            return Err("Display preview expired or the menu closed".into());
        }
        if let Err(error) = self.verify(&pending.output, pending.desired) {
            self.pending.as_mut().unwrap().confirmable = false;
            self.pending.as_mut().unwrap().deadline = Instant::now();
            let recovery = self.rollback();
            return Err(match recovery { Ok(()) => error, Err(r) => format!("{error}; rollback: {r}") });
        }
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
        if output.current_mode != Some(pending.previous) && output.current_mode != Some(pending.desired) {
            self.pending = None;
            return Err("Preview cancelled: display mode was changed elsewhere; leaving that mode untouched".into());
        }
        if output.current_mode != Some(pending.previous) {
            self.backend()?.set_mode(&output.name, pending.previous)?;
            self.verify(output, pending.previous)?;
        }
        self.pending = None;
        self.error.clear();
        Ok(())
    }
    fn load_saved(&mut self) -> Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        match fs::read(path) {
            Ok(bytes) => self.preferences = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Display preferences: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.preferences.clear(),
            Err(e) => return Err(format!("Display preferences: {e}")),
        }
        self.preferences_ready = true;
        Ok(())
    }
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
        fail_reads: AtomicBool,
        fail_after_set: AtomicBool,
    }
    impl Backend for Arc<Fake> {
        fn outputs(&self) -> Result<Vec<Output>> {
            if self.fail_reads.load(Ordering::SeqCst) { return Err("simulated IPC failure".into()); }
            Ok(self.outputs.lock().unwrap().clone())
        }
        fn set_mode(&self, name: &str, mode: Mode) -> Result<()> {
            self.commands.lock().unwrap().push((name.into(), mode));
            if !self.ignore_next.swap(false, Ordering::SeqCst) {
                let mut outputs = self.outputs.lock().unwrap();
                let output = outputs.iter_mut().find(|o| o.name == name)
                    .ok_or_else(|| "disconnected".to_string())?;
                output.current_mode = Some(mode);
            }
            if self.fail_after_set.swap(false, Ordering::SeqCst) {
                self.fail_reads.store(true, Ordering::SeqCst);
                return Err("simulated apply timeout".into());
            }
            Ok(())
        }
    }
    fn fixture() -> (Controller, Arc<Fake>, PathBuf) {
        let fake = Arc::new(Fake::default());
        fake.outputs.lock().unwrap().push(Output { name: "HDMI-A-1".into(),
            make: "Acer".into(), model: "XB273U V3".into(), serial: "serial-A".into(),
            active: true, current_mode: Some(ORIGINAL), modes: vec![ORIGINAL, TEST_MODE] });
        let directory = std::env::temp_dir().join(format!("zaman-display-{}-{}",
            std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        let state = Controller { backend: Some(Box::new(fake.clone())),
            path: Some(directory.join("display.json")), preferences: vec![], preferences_ready: true, pending: None,
            token: 100, error: String::new(), valid_menu: Arc::new(|g| g == 7) };
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
    fn keep_persists_but_loading_preferences_never_changes_a_mode() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let token = state.pending.as_ref().unwrap().token;
        state.confirm(7, token).unwrap();
        assert!(state.pending.is_none());
        let saved: Vec<Preference> = serde_json::from_slice(&fs::read(state.path.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(saved[0].mode, TEST_MODE);
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        let commands = fake.commands.lock().unwrap().len();
        state.load_saved().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        assert_eq!(fake.commands.lock().unwrap().len(), commands);
        fake.outputs.lock().unwrap()[0].serial = "another-display".into();
        fake.outputs.lock().unwrap()[0].current_mode = Some(ORIGINAL);
        state.load_saved().unwrap();
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
    #[test]
    fn tokens_are_json_strings_even_above_javascript_integer_precision() {
        let preview = Preview { token: u64::MAX, output: "HDMI-A-1".into(), mode: TEST_MODE,
            seconds_left: 15, confirmable: true };
        let value = serde_json::to_value(preview).unwrap();
        assert_eq!(value["token"].as_str(), Some("18446744073709551615"));
    }
    #[test]
    fn externally_changed_modes_are_neither_kept_nor_overwritten() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let external = Mode { refresh: 120000, ..ORIGINAL };
        fake.outputs.lock().unwrap()[0].current_mode = Some(external);
        let token = state.pending.as_ref().unwrap().token;
        assert!(state.confirm(7, token).is_err());
        assert!(state.pending.is_none());
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(external));
        assert_eq!(fake.commands.lock().unwrap().len(), 1);
        assert!(!directory.exists());
    }
    #[test]
    fn an_unverified_preview_cannot_be_kept() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        state.pending.as_mut().unwrap().confirmable = false;
        let token = state.pending.as_ref().unwrap().token;
        assert!(state.confirm(7, token).is_err());
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        assert!(!directory.exists());
    }
    #[test]
    fn preview_rejects_a_replacement_on_the_same_connector() {
        let (mut state, fake, _) = fixture();
        let old = fake.outputs().unwrap()[0].clone();
        fake.outputs.lock().unwrap()[0].serial = "replacement".into();
        assert!(state.preview_for(7, &old.name, TEST_MODE, Some(&old)).is_err());
        assert!(fake.commands.lock().unwrap().is_empty());
    }
    #[test]
    fn uncertain_apply_and_failed_recovery_never_make_keep_available() {
        let (mut state, fake, directory) = fixture();
        fake.fail_after_set.store(true, Ordering::SeqCst);
        assert!(state.preview(7, "HDMI-A-1", TEST_MODE).is_err());
        assert!(!state.pending.as_ref().unwrap().confirmable);
        let token = state.pending.as_ref().unwrap().token;
        assert!(state.confirm(7, token).is_err());
        assert!(!directory.exists());
        fake.fail_reads.store(false, Ordering::SeqCst);
        state.expire().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        assert!(state.pending.is_none());
    }
    #[test]
    fn reopening_rolls_back_the_old_generation_and_rejects_its_keep() {
        let (mut state, fake, directory) = fixture();
        let generation = Arc::new(AtomicU64::new(7));
        let valid = generation.clone();
        state.valid_menu = Arc::new(move |g| valid.load(Ordering::SeqCst) == g);
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let old_token = state.pending.as_ref().unwrap().token;
        generation.store(8, Ordering::SeqCst);
        state.expire().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        state.preview(8, "HDMI-A-1", TEST_MODE).unwrap();
        assert!(state.confirm(7, old_token).is_err());
        assert_eq!(state.pending.as_ref().unwrap().generation, 8);
        assert!(!directory.exists());
        state.rollback().unwrap();
    }
    #[test]
    fn corrupt_preferences_are_not_overwritten_by_keep() {
        let (mut state, fake, directory) = fixture();
        state.preview(7, "HDMI-A-1", TEST_MODE).unwrap();
        let token = state.pending.as_ref().unwrap().token;
        fs::create_dir_all(&directory).unwrap();
        fs::write(state.path.as_ref().unwrap(), b"not json").unwrap();
        assert!(state.confirm(7, token).is_err());
        assert_eq!(fs::read(state.path.as_ref().unwrap()).unwrap().as_slice(), b"not json");
        state.rollback().unwrap();
        assert_eq!(fake.outputs().unwrap()[0].current_mode, Some(ORIGINAL));
        fs::remove_dir_all(directory).unwrap();
    }
}
