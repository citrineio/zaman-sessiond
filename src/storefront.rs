//! Independently supervised storefronts; closing a game never stops these units.
use crate::command::CommandSpec;
use crate::error::{message, Result};
use crate::systemd::UserSystemd;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub fn unit(id: &str) -> Result<&'static str> {
    match id {
        "dukkan" => Ok("zaman-store-dukkan.service"),
        "steam" => Ok("zaman-store-steam.service"),
        _ => Err(message("unknown storefront; expected dukkan or steam")),
    }
}
#[derive(Deserialize)]
pub struct Config {
    pub argv: Vec<String>,
    #[serde(default)] pub env: BTreeMap<String, String>,
    pub workdir: Option<String>,
    #[serde(default)] pub native_guide: bool,
}
impl Config {
    pub fn load(id: &str) -> Result<Self> {
        unit(id)?;
        let root = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or_else(|| message("HOME/XDG_CONFIG_HOME is missing"))?;
        let path = root.join("zaman/storefronts.d").join(format!("{id}.toml"));
        let config: Self = toml::from_str(&std::fs::read_to_string(&path)
            .map_err(|e| message(format!("configure {} first: {e}", path.display())))?)?;
        if config.native_guide && id != "steam" { return Err(message("native Guide is supported only for Steam")); }
        Ok(config)
    }
    pub fn command(&self) -> Result<CommandSpec> {
        CommandSpec::new(self.argv.clone(), self.env.clone(), self.workdir.clone())
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub generation: u64,
    pub phase: String,
    pub native_guide: bool,
    pub game: bool,
    pub error: String,
}
#[derive(Default, Debug)]
struct State { next: u64, active: Option<String>, entries: BTreeMap<String, Entry> }
#[derive(Clone, Debug, Default)]
pub struct Storefronts { state: Arc<Mutex<State>> }
impl Storefronts {
    pub fn active(&self) -> Option<String> { self.state.lock().unwrap().active.clone() }
    pub fn entry(&self, id: &str) -> Option<Entry> { self.state.lock().unwrap().entries.get(id).cloned() }
    pub fn native_guide(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.active.as_ref().and_then(|id| s.entries.get(id)).is_some_and(|e| e.native_guide && e.phase == "Running")
    }
    pub fn has_game(&self) -> bool { self.state.lock().unwrap().entries.values().any(|e| e.game) }
    pub fn reserve(&self, id: &str, native_guide: bool) -> Result<u64> {
        unit(id)?;
        let mut s = self.state.lock().unwrap();
        if s.entries.get(id).is_some_and(|e| e.phase != "Failed" && e.phase != "Exited") {
            return Err(message("storefront already running or changing state"));
        }
        s.next = s.next.wrapping_add(1).max(1);
        let generation = s.next;
        s.entries.insert(id.into(), Entry { generation, phase: "Starting".into(), native_guide, game: false, error: String::new() });
        s.active = Some(id.into());
        Ok(generation)
    }
    pub fn activate(&self, id: &str) -> Result<()> {
        let mut s = self.state.lock().unwrap();
        if !s.entries.get(id).is_some_and(|e| e.phase == "Running") { return Err(message("storefront is not ready")); }
        s.active = Some(id.into());
        Ok(())
    }
    pub fn home(&self) { self.state.lock().unwrap().active = None; }
    /// Closing Steam is also its confirmed game/client recovery action.
    /// Home remains blocked while any storefront game is alive.
    pub fn validate_leave(&self, id: &str, generation: u64, close: bool) -> Result<()> {
        unit(id)?;
        let s = self.state.lock().unwrap();
        let e = s.entries.get(id).ok_or_else(|| message("no such storefront session"))?;
        if generation != e.generation || s.active.as_deref() != Some(id) {
            return Err(message("stale storefront session"));
        }
        if s.entries.values().any(|e| e.game) && !(close && id == "steam") {
            return Err(message("close the game in its storefront before returning to library or closing the client"));
        }
        if close && !matches!(e.phase.as_str(), "Starting" | "Running") {
            return Err(message("storefront is already closing"));
        }
        Ok(())
    }
    pub fn update(&self, id: &str, generation: u64, phase: &str, error: String) {
        let mut s = self.state.lock().unwrap();
        if let Some(e) = s.entries.get_mut(id).filter(|e| e.generation == generation) {
            e.phase = phase.into(); e.error = error;
            if matches!(phase, "Failed" | "Exited") { e.game = false; if s.active.as_deref() == Some(id) { s.active = None; } }
        }
    }
    pub fn set_game(&self, id: &str, game: bool) {
        if let Some(e) = self.state.lock().unwrap().entries.get_mut(id) { e.game = game; }
    }
    // Additive API: id, generation, phase, Guide owner, game running, last error.
    pub fn context(&self) -> (String, u64, String, String, bool, String) {
        let s = self.state.lock().unwrap();
        let Some(id) = &s.active else { return (String::new(), 0, "Idle".into(), "zaman".into(), false, String::new()); };
        let e = &s.entries[id];
        (id.clone(), e.generation, e.phase.clone(), if e.native_guide { "steam" } else { "zaman" }.into(), e.game, e.error.clone())
    }
}

pub async fn run(id: String, generation: u64, command: CommandSpec, state: Storefronts,
    compositor: crate::sway::SwayController, mut stop: mpsc::Receiver<()>) -> (String, u64, Result<()>) {
    let result = async {
        let systemd = UserSystemd::connect().await?;
        // This is the final gate before executing the client. The API worker
        // may already have activated the store, but its task can run after this
        // one. Requiring the workspace here closes that scheduling race.
        if id == "steam" { compositor.activate_storefront(Some(id.clone())).await?; }
        let handle = systemd.launch_named(&command, unit(&id)?, true).await?;
        let setup = async {
            let cgroup = systemd.control_group(&handle).await?;
            compositor.storefront_started(id.clone(), cgroup).await
        }.await;
        if let Err(error) = setup {
            systemd.stop(&handle).await?;
            return Err(error);
        }
        state.update(&id, generation, "Running", String::new());
        tokio::select! {
            result = systemd.wait_for_exit(&handle) => {
                if let Err(error) = result { systemd.stop(&handle).await?; return Err(error); }
                Ok(())
            },
            _ = stop.recv() => systemd.stop(&handle).await,
        }
    }.await;
    if let Err(error) = &result {
        state.update(&id, generation, "Stopping", error.to_string());
        loop {
            let cleanup = async { UserSystemd::connect().await?.stop_storefront(&id).await }.await;
            if cleanup.is_ok() { break; }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
    (id, generation, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn old_completion_does_not_clear_replacement() {
        let s = Storefronts::default(); let first = s.reserve("steam", false).unwrap();
        s.update("steam", first, "Exited", String::new());
        let second = s.reserve("steam", true).unwrap();
        s.update("steam", first, "Failed", "stale".into());
        assert_eq!(s.context().1, second); assert_eq!(s.context().2, "Starting");
    }
    #[test] fn home_preserves_client_and_detected_game() {
        let s = Storefronts::default(); let g = s.reserve("steam", false).unwrap();
        s.update("steam", g, "Running", String::new()); s.set_game("steam", true); s.home();
        assert!(s.has_game()); assert_eq!(s.entry("steam").unwrap().phase, "Running");
        s.activate("steam").unwrap(); assert!(s.context().4);
    }
    #[test] fn registry_is_allowlisted() { assert!(unit("../bad").is_err()); assert!(unit("steam").is_ok()); }
    #[test] fn steam_game_allows_close_but_blocks_home_without_mutation() {
        let s = Storefronts::default(); let g = s.reserve("steam", true).unwrap();
        s.update("steam", g, "Running", String::new()); s.set_game("steam", true);
        let before = s.context();
        assert!(s.validate_leave("steam", g, true).is_ok());
        assert!(s.validate_leave("steam", g, false).is_err());
        assert_eq!(s.context(), before);
    }
    #[test] fn leave_rejects_stale_background_and_duplicate_close() {
        let s = Storefronts::default(); let old = s.reserve("steam", true).unwrap();
        s.update("steam", old, "Exited", String::new());
        let g = s.reserve("steam", true).unwrap(); s.set_game("steam", true);
        assert!(s.validate_leave("steam", old, true).is_err());
        s.home(); assert!(s.validate_leave("steam", g, true).is_err());
        s.update("steam", g, "Running", String::new()); s.activate("steam").unwrap();
        s.update("steam", g, "Stopping", String::new());
        assert!(s.validate_leave("steam", g, true).is_err());
        assert_eq!(s.entry("steam").unwrap().phase, "Stopping");
    }
    #[test] fn steam_recovery_does_not_allow_other_client_to_close_over_game() {
        let s = Storefronts::default(); s.reserve("steam", true).unwrap(); s.set_game("steam", true);
        let d = s.reserve("dukkan", false).unwrap();
        assert!(s.validate_leave("dukkan", d, true).is_err());
        assert!(s.validate_leave("dukkan", d, false).is_err());
        s.set_game("steam", false);
        assert!(s.validate_leave("dukkan", d, true).is_ok());
        assert!(s.validate_leave("dukkan", d, false).is_ok());
    }
}
