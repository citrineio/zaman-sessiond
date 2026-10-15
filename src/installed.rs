//! Curated user Flatpak games, independently supervised from the Dukkan client.
use crate::command::CommandSpec;
use crate::error::{message, Result};
use crate::session::{SessionControl, SessionOutcome};
use crate::systemd::UserSystemd;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Instant};

pub const UNIT: &str = "zaman-installed-game.service";
const APPS: &[(&str, &str)] = &[
    ("org.supertuxproject.SuperTux", "SuperTux"),
    ("net.supertuxkart.SuperTuxKart", "SuperTuxKart"),
    ("org.wesnoth.Wesnoth", "The Battle for Wesnoth"),
    ("org.openttd.OpenTTD", "OpenTTD"),
    ("org.opensurge2d.OpenSurge", "Surge Engine"),
];

pub fn name(id: &str) -> Result<&'static str> {
    APPS.iter().find(|(app, _)| *app == id).map(|(_, name)| *name)
        .ok_or_else(|| message("application is outside the curated installed-game catalog"))
}

fn control_outcome(control: SessionControl) -> SessionOutcome {
    match control {
        SessionControl::Stop => SessionOutcome::StopRequested,
        SessionControl::ExitGame => SessionOutcome::ExitGameRequested,
        SessionControl::Reboot => SessionOutcome::RebootRequested,
        SessionControl::Shutdown => SessionOutcome::ShutdownRequested,
    }
}

fn helper_path() -> PathBuf {
    std::env::var_os("ZAMAN_INSTALLED_GAME_HELPER").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/local/libexec/zaman/flatpak-game-session.py"))
}

fn state_path() -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| message("XDG_RUNTIME_DIR is missing"))?;
    Ok(PathBuf::from(runtime).join("zaman-installed-game/session.json"))
}

fn close_path() -> Result<PathBuf> {
    Ok(state_path()?.with_file_name("close.json"))
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub generation: u64,
    pub app: String,
    pub name: String,
    pub phase: String,
    pub error: String,
    pub ready: bool,
}

#[derive(Debug, Default)]
struct State { next: u64, current: Option<Entry> }

#[derive(Clone, Debug, Default)]
pub struct InstalledGames { state: Arc<Mutex<State>> }

impl InstalledGames {
    pub fn snapshot(&self) -> Option<Entry> { self.state.lock().unwrap().current.clone() }
    pub fn busy(&self) -> bool {
        self.snapshot().is_some_and(|e| matches!(e.phase.as_str(),
            "Starting" | "Running" | "Stopping" | "CleanupFailed"))
    }
    pub fn reserve(&self, app: &str) -> Result<Entry> {
        let title = name(app)?;
        let mut state = self.state.lock().unwrap();
        if state.current.as_ref().is_some_and(|e| matches!(e.phase.as_str(),
            "Starting" | "Running" | "Stopping" | "CleanupFailed")) {
            return Err(message("an installed game is already active or needs cleanup"));
        }
        state.next = state.next.wrapping_add(1).max(1);
        let entry = Entry { generation: state.next, app: app.into(), name: title.into(),
            phase: "Starting".into(), error: String::new(), ready: false };
        state.current = Some(entry.clone());
        Ok(entry)
    }
    pub fn update(&self, generation: u64, phase: &str, error: String, ready: bool) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.current.as_mut().filter(|e| e.generation == generation) {
            entry.phase = phase.into(); entry.error = error; entry.ready = ready;
        }
    }
    pub fn context(&self) -> (String, String, String, String, String, bool) {
        match self.snapshot() {
            Some(e) => (e.generation.to_string(), e.app, e.name, e.phase, e.error, e.ready),
            None => ("0".into(), String::new(), String::new(), "Idle".into(), String::new(), false),
        }
    }
}

#[derive(Deserialize)]
struct Record {
    app: String,
    nonce: String,
    phase: String,
    cgroup: Option<String>,
    #[serde(default)] windows: Vec<serde_json::Value>,
    error: Option<String>,
}

fn record() -> Result<Record> {
    let path = state_path()?;
    if path.is_symlink() { return Err(message("unsafe installed-game state link")); }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn matching_record(app: &str, nonce: &str) -> Result<Option<Record>> {
    match record() {
        Ok(record) if record.app == app && record.nonce == nonce => Ok(Some(record)),
        Ok(_) => Ok(None),
        Err(_) => Ok(None),
    }
}

fn request_close(nonce: &str) -> Result<()> {
    let path = close_path()?;
    let temporary = path.with_file_name(format!("close.{nonce}.tmp"));
    fs::write(&temporary, format!("{{\"nonce\":\"{nonce}\"}}\n"))?;
    fs::rename(temporary, path)?;
    Ok(())
}

async fn recover_helper() -> Result<()> {
    let helper = helper_path();
    let output = tokio::task::spawn_blocking(move || std::process::Command::new("/usr/bin/python3")
        .arg(helper).arg("recover-managed").output()).await??;
    if output.status.success() { Ok(()) } else {
        Err(message(format!("installed-game recovery failed: {}", String::from_utf8_lossy(&output.stderr))))
    }
}

pub async fn recover_startup(systemd: &UserSystemd) -> Result<()> {
    systemd.stop_installed_game().await?;
    if state_path()?.exists() { recover_helper().await } else { Ok(()) }
}

pub async fn retry_cleanup() -> Result<()> {
    let systemd = UserSystemd::connect().await?;
    recover_startup(&systemd).await
}

pub async fn run(entry: Entry, state: InstalledGames, compositor: crate::sway::SwayController,
    mut controls: mpsc::Receiver<SessionControl>) -> (u64, Result<SessionOutcome>) {
    let generation = entry.generation;
    let mut cleanup_proven = true;
    let result = async {
        let helper = helper_path();
        if !helper.is_absolute() || !helper.is_file() {
            return Err(message(format!("installed-game helper is missing: {}", helper.display())));
        }
        let nonce = fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim().replace('-', "");
        if nonce.len() != 32 { return Err(message("could not create launch nonce")); }
        let command = CommandSpec::new(vec!["/usr/bin/python3".into(),
            helper.to_string_lossy().into_owned(), "run-managed".into(), entry.app.clone(), nonce.clone()],
            BTreeMap::new(), None)?;
        let systemd = UserSystemd::connect().await?;
        cleanup_proven = false;
        let handle = match systemd.launch_named(&command, UNIT, false).await {
            Ok(handle) => handle,
            Err(error) => {
                let cleanup = recover_startup(&systemd).await;
                cleanup_proven = cleanup.is_ok();
                return Err(match cleanup { Ok(()) => error, Err(other) => message(format!("{error}; cleanup: {other}")) });
            }
        };
        let mut cancelled = None;
        let launched = async {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut registered = false;
            loop {
                if let Some(record) = matching_record(&entry.app, &nonce)? {
                    if record.phase == "Running" {
                        if !registered {
                            let group = record.cgroup.ok_or_else(|| message("managed Flatpak record lacks its verified scope"))?;
                            compositor.game_started(group).await?;
                            registered = true;
                        }
                        if !record.windows.is_empty() {
                            state.update(generation, "Running", String::new(), true);
                            break Ok(());
                        }
                    }
                    if matches!(record.phase.as_str(), "Exited" | "Failed" | "CleanupFailed") {
                        break Err(message(record.error.unwrap_or_else(|| "game exited during startup".into())));
                    }
                }
                if let Ok(control) = controls.try_recv() {
                    cancelled = Some(control);
                    break Err(message("installed game launch was cancelled"));
                }
                if Instant::now() >= deadline { break Err(message("installed game readiness timed out")); }
                sleep(Duration::from_millis(100)).await;
            }
        }.await;
        if let Err(error) = launched {
            let cleanup = recover_startup(&systemd).await;
            cleanup_proven = cleanup.is_ok();
            if cleanup_proven {
                if let Some(control) = cancelled { return Ok(control_outcome(control)); }
            }
            return Err(match cleanup { Ok(()) => error, Err(other) => message(format!("{error}; cleanup: {other}")) });
        }
        let outcome: Result<SessionOutcome> = tokio::select! {
            result = systemd.wait_for_exit(&handle) => result.map(|_| SessionOutcome::GameExited),
            control = controls.recv() => {
                let control = control.unwrap_or(SessionControl::Stop);
                state.update(generation, "Stopping", String::new(), false);
                async {
                    request_close(&nonce)?;
                    timeout(Duration::from_secs(25), systemd.wait_for_exit(&handle)).await
                        .map_err(|_| message("managed game close timed out"))??;
                    Ok(control_outcome(control))
                }.await
            }
        };
        let outcome = match (outcome, matching_record(&entry.app, &nonce)?) {
            (Ok(value), Some(record)) if record.phase == "Exited" => Ok(value),
            (Ok(_), Some(record)) => {
                let phase = record.phase;
                Err(message(record.error.unwrap_or_else(|| format!("managed Flatpak helper ended in {phase}"))))
            }
            (Ok(_), None) => Err(message("managed Flatpak helper exited without its matching record")),
            (result, _) => result,
        };
        let cleanup = recover_startup(&systemd).await;
        cleanup_proven = cleanup.is_ok();
        match (outcome, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(first), Err(second)) => Err(message(format!("{first}; cleanup: {second}"))),
        }
    }.await;
    match &result {
        Ok(_) => state.update(generation, "Exited", String::new(), false),
        Err(error) => state.update(generation,
            if cleanup_proven { "Failed" } else { "CleanupFailed" }, error.to_string(), false),
    }
    (generation, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn catalog_rejects_untrusted_argv() {
        assert!(name("net.supertuxkart.SuperTuxKart").is_ok());
        assert!(name("../../flatpak").is_err());
    }
    #[test] fn cleanup_failed_blocks_replacement() {
        let state = InstalledGames::default();
        let entry = state.reserve("org.openttd.OpenTTD").unwrap();
        state.update(entry.generation, "CleanupFailed", "uncertain".into(), false);
        assert!(state.reserve("org.wesnoth.Wesnoth").is_err());
    }
}
