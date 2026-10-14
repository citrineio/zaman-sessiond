//! Sway policy for the supervised game and the fallback xdg-shell menu.
//! IPC runs on blocking workers, never on the single-threaded D-Bus executor.
use crate::error::{message, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LIBRARY: &str = "library";
const GAME: &str = "game";
const PEGASUS: &str = "org.pegasus-frontend.pegasus-fe";
const MENU: &str = "zaman-menu";

#[derive(Debug, Default, Deserialize)]
struct Node {
    #[serde(default)] id: u64,
    #[serde(default, rename = "type")] kind: String,
    #[serde(default)] name: Option<String>,
    #[serde(default)] app_id: Option<String>,
    #[serde(default)] pid: Option<u32>,
    #[serde(default)] focused: bool,
    #[serde(default)] fullscreen_mode: u8,
    #[serde(default)] border: String,
    #[serde(default)] window_properties: Option<WindowProperties>,
    #[serde(default)] nodes: Vec<Node>,
    #[serde(default)] floating_nodes: Vec<Node>,
}

#[derive(Debug, Default, Deserialize)]
struct WindowProperties { #[serde(default)] class: Option<String> }

#[derive(Clone, Debug)]
struct View {
    id: u64,
    pid: Option<u32>,
    app_id: Option<String>,
    workspace: String,
    focused: bool,
    fullscreen: Option<(u64, u8)>,
    floating: bool,
    border: String,
    steam_game: bool,
}

impl Node {
    fn contains(&self, id: u64) -> bool {
        self.id == id || self.nodes.iter().chain(&self.floating_nodes).any(|n| n.contains(id))
    }
    fn views(&self) -> Vec<View> {
        fn walk(node: &Node, workspace: &str, fullscreen: Option<(u64, u8)>, floating: bool, out: &mut Vec<View>) {
            let workspace = if node.kind == "workspace" {
                node.name.as_deref().unwrap_or(workspace)
            } else { workspace };
            // Workspace metadata reports mode 1 even with no fullscreen view.
            // Only real containers can own fullscreen and match con_id.
            let fullscreen = if matches!(node.kind.as_str(), "con" | "floating_con") && node.fullscreen_mode != 0 {
                Some((node.id, node.fullscreen_mode))
            } else { fullscreen };
            // pid covers native Wayland and Xwayland, regardless of app_id.
            if node.pid.is_some() || node.app_id.is_some() {
                out.push(View { id: node.id, pid: node.pid, app_id: node.app_id.clone(),
                    workspace: workspace.into(), focused: node.focused, fullscreen,
                    floating, border: node.border.clone(), steam_game: node.window_properties.as_ref().and_then(|p| p.class.as_deref()).is_some_and(steam_game_class) });
            }
            for child in &node.nodes {
                walk(child, workspace, fullscreen, floating, out);
            }
            for child in &node.floating_nodes {
                walk(child, workspace, fullscreen, true, out);
            }
        }
        let mut views = Vec::new();
        walk(self, "", None, false, &mut views);
        views
    }
}

trait Backend: Send + Sync + Debug {
    fn tree(&self) -> Result<Node>;
    fn command(&self, command: &str) -> Result<()>;
    fn owns_pid(&self, pid: u32, cgroup: &str) -> bool;
    fn steam_game_pid(&self, _pid: u32) -> bool { false }
}

#[derive(Debug)]
struct Native { socket: PathBuf }
impl Native {
    fn request(&self, kind: u32, payload: &[u8]) -> Result<Vec<u8>> {
        let mut socket = UnixStream::connect(&self.socket)?;
        let timeout = Some(Duration::from_millis(750));
        socket.set_read_timeout(timeout)?;
        socket.set_write_timeout(timeout)?;
        socket.write_all(b"i3-ipc")?;
        socket.write_all(&(payload.len() as u32).to_ne_bytes())?;
        socket.write_all(&kind.to_ne_bytes())?;
        socket.write_all(payload)?;
        let mut header = [0; 14];
        socket.read_exact(&mut header)?;
        let size = u32::from_ne_bytes(header[6..10].try_into()?) as usize;
        let reply_kind = u32::from_ne_bytes(header[10..14].try_into()?);
        if &header[..6] != b"i3-ipc" || reply_kind != kind || size > 8 * 1024 * 1024 {
            return Err(message("invalid Sway IPC response"));
        }
        let mut reply = vec![0; size];
        socket.read_exact(&mut reply)?;
        Ok(reply)
    }
}
impl Backend for Native {
    fn tree(&self) -> Result<Node> { Ok(serde_json::from_slice(&self.request(4, b"")?)?) }
    fn command(&self, command: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Reply { success: bool, #[serde(default)] error: String }
        let replies: Vec<Reply> = serde_json::from_slice(&self.request(0, command.as_bytes())?)?;
        if replies.is_empty() { return Err(message("empty Sway command reply")); }
        for reply in replies {
            if !reply.success { return Err(message(format!("Sway command {command:?}: {}", reply.error))); }
        }
        Ok(())
    }
    fn steam_game_pid(&self, pid: u32) -> bool {
        std::fs::read(format!("/proc/{pid}/environ")).map(|env| steam_game_environment(&env)).unwrap_or(false)
    }
    fn owns_pid(&self, pid: u32, cgroup: &str) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
            .map(|text| in_cgroup(&text, cgroup)).unwrap_or(false)
    }
}

fn steam_game_environment(env: &[u8]) -> bool {
    env.split(|b| *b == 0).any(|field| field.strip_prefix(b"SteamAppId=")
        .and_then(|id| std::str::from_utf8(id).ok())
        .and_then(|id| id.parse::<u64>().ok()).is_some_and(|id| id != 0))
}

fn steam_game_class(class: &str) -> bool {
    class.strip_prefix("steam_app_").is_some_and(|id|
        !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && id.parse::<u64>().is_ok_and(|id| id != 0))
}

fn in_cgroup(text: &str, expected: &str) -> bool {
    if expected.is_empty() || expected == "/" { return false; }
    text.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next();
        let controllers = fields.next();
        let path = fields.next().unwrap_or("");
        // Unified hierarchy, or the v1 systemd hierarchy. Do not accidentally
        // accept membership in a different resource controller's root group.
        let relevant = (hierarchy == Some("0") && controllers == Some(""))
            || controllers.is_some_and(|s| s.split(',').any(|c| c == "name=systemd"));
        relevant && (path == expected
            || path.strip_prefix(expected).is_some_and(|tail| tail.starts_with('/')))
    })
}

#[derive(Clone, Debug)]
struct SavedFullscreen { owner: u64, mode: u8, member: u64, pid: Option<u32> }
#[derive(Debug)]
struct Lease {
    return_view: Option<(u64, Option<u32>)>,
    return_game: bool,
    fullscreen: BTreeMap<u64, SavedFullscreen>,
}
#[derive(Debug)]
struct TrackedGame { cgroup: String, primary: Option<u64> }
#[derive(Debug)]
struct State {
    backend: Option<Arc<dyn Backend>>,
    game: Option<TrackedGame>,
    stores: BTreeMap<String, TrackedGame>,
    active_store: Option<String>,
    restore_store_focus: bool,
    lease: Option<Lease>,
    restore_library: bool,
}

impl State {
    fn views(&self) -> Result<Vec<View>> {
        match &self.backend { Some(b) => Ok(b.tree()?.views()), None => Ok(vec![]) }
    }
    fn command(&self, id: u64, command: &str) -> Result<()> {
        match &self.backend {
            Some(b) => b.command(&format!("[con_id={id}] {command}")),
            None => Ok(()),
        }
    }
    fn owns(&self, view: &View) -> bool {
        match (&self.backend, &self.game, view.pid) {
            (Some(b), Some(g), Some(pid)) => b.owns_pid(pid, &g.cgroup),
            _ => false,
        }
    }
    fn place_library(&self) -> Result<()> {
        for view in self.views()?.iter().filter(|v| v.app_id.as_deref() == Some(PEGASUS)) {
            // Pegasus is deliberately tiled and never compositor-fullscreen.
            if view.fullscreen.is_some() {
                self.command(view.fullscreen.unwrap().0, "fullscreen disable")?;
            }
            if view.floating || view.border != "none" {
                self.command(view.id, "floating disable, border none")?;
            }
            if view.workspace != LIBRARY {
                self.command(view.id, "move container to workspace \"library\"")?;
            }
        }
        Ok(())
    }
    fn focus_library(&self) -> Result<()> {
        self.place_library()?;
        if let Some(view) = self.views()?.iter().find(|v| v.app_id.as_deref() == Some(PEGASUS)) {
            self.command(view.id, "focus")?;
        } else if let Some(backend) = &self.backend {
            backend.command("workspace \"library\"")?;
        }
        Ok(())
    }
    fn suspend(&mut self, view: &View, desired: Option<(u64, u8)>) -> Result<()> {
        let Some((owner, mode)) = desired.or(view.fullscreen) else { return Ok(()) };
        if let Some(lease) = &mut self.lease {
            // Save before the command: a timeout can follow a successful apply.
            lease.fullscreen.entry(owner).or_insert(SavedFullscreen {
                owner, mode, member: view.id, pid: view.pid,
            });
        }
        if view.fullscreen.is_some() { self.command(owner, "fullscreen disable")?; }
        Ok(())
    }
    fn store_owns(&self, id: &str, view: &View) -> bool {
        match (&self.backend, self.stores.get(id), view.pid) {
            (Some(b), Some(g), Some(pid)) => b.owns_pid(pid, &g.cgroup),
            _ => false,
        }
    }
    fn store_game(&self, id: &str, view: &View) -> bool {
        id == "steam" && (view.steam_game || view.pid.is_some_and(|pid|
            self.backend.as_ref().is_some_and(|b| b.steam_game_pid(pid))))
    }
    fn route_stores(&mut self) -> Result<()> {
        for id in self.stores.keys().cloned().collect::<Vec<_>>() {
            let views: Vec<_> = self.views()?.into_iter().filter(|v| self.store_owns(&id, v)).collect();
            let previous = self.stores[&id].primary;
            // Prefer the game's window over the Steam client while it is mapped.
            let primary = views.iter().filter(|v| self.store_game(&id, v)).find(|v| v.focused)
                .or_else(|| views.iter().find(|v| self.store_game(&id, v)))
                .or_else(|| views.iter().find(|v| v.focused && (!v.floating || v.fullscreen.is_some())))
                .or_else(|| views.iter().find(|v| Some(v.id) == previous))
                .or_else(|| views.iter().find(|v| v.fullscreen.is_some()))
                .or_else(|| views.first());
            let workspace = format!("store-{id}");
            for view in &views {
                if view.workspace != workspace {
                    self.command(view.id, &format!("move container to workspace \"{workspace}\""))?;
                }
            }
            if let Some(view) = primary {
                // The layer-shell menu does not require removing fullscreen.
                // Repair fullscreen after client/window changes and Guide cycles.
                if previous != Some(view.id) || view.floating || view.border != "none" {
                    self.command(view.id, "floating disable, border none")?;
                }
                // Steam and its games own their fullscreen requests. Big Picture
                // can fill its dedicated workspace as a tiled window, matching
                // the validated Pegasus launch path without an extra transition.
                if id != "steam" && view.fullscreen.is_none() {
                    self.command(view.id, "fullscreen enable")?;
                }
                if self.active_store.as_deref() == Some(id.as_str()) && self.game.is_none() && self.lease.is_none()
                    && (previous != Some(view.id) || self.restore_store_focus) {
                    self.command(view.id, "focus")?;
                    self.restore_store_focus = false;
                }
                self.stores.get_mut(&id).unwrap().primary = Some(view.id);
            }
        }
        Ok(())
    }
    fn activate_store(&mut self, id: Option<String>) -> Result<()> {
        if let Some(id) = &id {
            crate::storefront::unit(id)?;
            if id == "steam" {
                if let Some(backend) = &self.backend {
                    // Choose the destination before a client is executed or focused.
                    // A later move can leave Pegasus sharing the launch workspace
                    // during initial mapping.
                    backend.command(&format!("workspace --no-auto-back-and-forth \"store-{id}\""))?;
                }
            }
        }
        self.active_store = id.clone();
        self.restore_store_focus = id.is_some();
        self.route_stores()?;
        if id.is_none() {
            self.restore_library = true;
            self.focus_library()?;
            self.restore_library = false;
        }
        Ok(())
    }

    fn route_game(&mut self) -> Result<()> {
        if self.game.is_none() { return Ok(()) }
        let views: Vec<View> = self.views()?.into_iter().filter(|v| self.owns(v)).collect();
        if views.is_empty() { return Ok(()) }
        let previous = self.game.as_ref().and_then(|g| g.primary);
        let primary = views.iter().find(|v| v.focused)
            .or_else(|| views.iter().find(|v| Some(v.id) == previous))
            .or_else(|| views.iter().find(|v| v.fullscreen.is_some()))
            .unwrap_or(&views[0]).clone();
        let changed = previous != Some(primary.id);
        for view in &views {
            if view.workspace != GAME {
                self.command(view.id, "move container to workspace \"game\"")?;
            }
            if self.lease.is_some() { self.suspend(view, None)?; }
        }
        if changed {
            self.command(primary.id, "floating disable, border none")?;
            if let Some(lease) = &mut self.lease {
                if lease.return_game { lease.return_view = Some((primary.id, primary.pid)); }
                // A window first mapped during the menu still starts in the
                // normal fullscreen game policy once that menu closes.
                self.suspend(&primary, primary.fullscreen.or(Some((primary.id, 1))))?;
            } else {
                self.command(primary.id, "fullscreen enable, focus")?;
            }
            self.game.as_mut().unwrap().primary = Some(primary.id);
            println!("Sway game window={} workspace=game", primary.id);
        }
        if self.lease.is_some() {
            for view in self.views()?.iter().filter(|v| v.app_id.as_deref() == Some(MENU)) {
                if view.workspace == "__i3_scratch" { continue; }
                if view.workspace != GAME {
                    self.command(view.id, "move container to workspace \"game\"")?;
                }
            }
        }
        Ok(())
    }
    fn prepare_menu(&mut self, game_active: bool) -> Result<()> {
        if self.backend.is_none() { return Ok(()) }
        if self.lease.is_some() { self.finish_menu(game_active)?; }
        self.place_library()?;
        self.route_game()?;
        let views = self.views()?;
        let target = if game_active {
            views.iter().find(|v| v.focused && self.owns(v))
                .or_else(|| views.iter().find(|v| self.owns(v)
                    && self.game.as_ref().is_some_and(|g| g.primary == Some(v.id))))
                .or_else(|| views.iter().find(|v| self.owns(v)))
        } else {
            views.iter().find(|v| v.focused && v.app_id.as_deref() != Some(MENU))
                .or_else(|| views.iter().find(|v| v.app_id.as_deref() == Some(PEGASUS)))
        };
        self.lease = Some(Lease { return_view: target.map(|v| (v.id, v.pid)),
            return_game: game_active, fullscreen: BTreeMap::new() });
        for view in &views {
            if (game_active && self.owns(view)) || target.is_some_and(|t| t.id == view.id) {
                self.suspend(view, None)?;
            }
        }
        if let Some(view) = target { self.command(view.id, "focus")?; }
        Ok(())
    }
    fn focus_menu(&self) -> Result<()> {
        if self.backend.is_none() { return Ok(()) }
        let views = self.views()?;
        let view = views.iter().filter(|v| v.app_id.as_deref() == Some(MENU)
            && v.workspace != "__i3_scratch")
            .max_by_key(|v| v.id).ok_or_else(|| message("menu acknowledged, but Sway has no menu window"))?;
        self.command(view.id, "floating enable, border none, resize set width 46 ppt height 70 ppt, move position center")?;
        self.command(view.id, "focus")
    }
    fn keep_menu_focused(&self) -> Result<()> {
        let views = self.views()?;
        // Mapping/geometry changes or pointer focus can return focus to the
        // background. A modal menu owns focus over both Pegasus and games.
        if let Some(view) = views.iter().filter(|v| v.app_id.as_deref() == Some(MENU)
            && v.workspace != "__i3_scratch").max_by_key(|v| v.id) {
            if !view.focused { self.command(view.id, "focus")?; }
        }
        Ok(())
    }
    fn finish_menu(&mut self, game_active: bool) -> Result<()> {
        if self.backend.is_none() || self.lease.is_none() { return Ok(()) }
        let tree = self.backend.as_ref().unwrap().tree()?;
        let views = tree.views();
        // Hide the ordinary surface before restoring fullscreen/focus/input.
        // MenuClosed will destroy it; this also covers an unresponsive renderer.
        for view in views.iter().filter(|v| v.app_id.as_deref() == Some(MENU)) {
            if view.workspace != "__i3_scratch" { self.command(view.id, "move scratchpad")?; }
        }
        let lease = self.lease.as_ref().unwrap();
        for saved in lease.fullscreen.values() {
            if lease.return_game && !game_active { continue; }
            // Sway IDs plus PID identity prevent restoring a vanished target.
            if tree.contains(saved.owner) && views.iter().any(|v| v.id == saved.member && v.pid == saved.pid) {
                let command = if saved.mode == 2 { "fullscreen enable global" } else { "fullscreen enable" };
                self.command(saved.owner, command)?;
            }
        }
        let target = lease.return_view.and_then(|(id, pid)| {
            views.iter().find(|v| v.id == id && v.pid == pid
                && (!lease.return_game || (game_active && self.owns(v))))
        });
        if let Some(view) = target {
            self.command(view.id, "focus")?;
        } else if game_active && lease.return_game {
            if let Some(view) = views.iter().find(|v| self.owns(v)
                && self.game.as_ref().is_some_and(|g| g.primary == Some(v.id))) {
                self.command(view.id, "focus")?;
            } else { self.focus_library()?; }
        } else { self.focus_library()?; }
        // Keep the lease on failure so periodic reconciliation can retry.
        self.lease = None;
        Ok(())
    }
    fn game_ended(&mut self) -> Result<()> {
        self.game = None;
        self.restore_library = true;
        self.finish_menu(false)?;
        self.focus_library()?;
        self.restore_library = false;
        Ok(())
    }
    fn reconcile(&mut self, menu_open: bool, game_active: bool) -> Result<()> {
        if self.backend.is_none() { return Ok(()) }
        if self.restore_library {
            self.finish_menu(false)?;
            self.focus_library()?;
            self.restore_library = false;
        }
        if !menu_open && self.lease.is_some() { self.finish_menu(game_active)?; }
        self.place_library()?;
        self.route_game()?;
        self.route_stores()?;
        if menu_open && self.lease.is_some() { self.keep_menu_focused()?; }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SwayController { state: Arc<Mutex<State>>, enabled: bool }
impl SwayController {
    pub fn from_environment() -> Self {
        let backend = std::env::var_os("SWAYSOCK").filter(|s| !s.is_empty())
            .map(|socket| Arc::new(Native { socket: socket.into() }) as Arc<dyn Backend>);
        Self { enabled: backend.is_some(), state: Arc::new(Mutex::new(State {
            backend, game: None, stores: BTreeMap::new(), active_store: None, restore_store_focus: false, lease: None, restore_library: false })) }
    }
    pub fn enabled(&self) -> bool { self.enabled }
    async fn run(&self, operation: impl FnOnce(&mut State) -> Result<()> + Send + 'static) -> Result<()> {
        if !self.enabled { return Ok(()) }
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            operation(&mut state.lock().unwrap_or_else(|e| e.into_inner()))
        }).await?
    }
    pub async fn storefront_started(&self, id: String, cgroup: String) -> Result<()> {
        self.run(move |s| {
            crate::storefront::unit(&id)?;
            if !cgroup.starts_with('/') || cgroup == "/" || cgroup.ends_with('/') {
                return Err(message("invalid storefront control group"));
            }
            s.stores.insert(id, TrackedGame { cgroup, primary: None });
            Ok(())
        }).await
    }
    pub async fn activate_storefront(&self, id: Option<String>) -> Result<()> {
        self.run(move |s| s.activate_store(id)).await
    }
    pub async fn storefront_ended(&self, id: String) -> Result<()> {
        self.run(move |s| {
            s.stores.remove(&id);
            if s.active_store.as_deref() == Some(id.as_str()) { s.activate_store(None)?; }
            Ok(())
        }).await
    }
    pub async fn steam_game_running(&self) -> Result<bool> {
        if !self.enabled { return Ok(false); }
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let s = state.lock().unwrap_or_else(|e| e.into_inner());
            Ok(s.views()?.iter().any(|v| s.store_owns("steam", v) && s.store_game("steam", v)))
        }).await?
    }
    pub async fn game_started(&self, cgroup: String) -> Result<()> {
        self.run(move |s| {
            if !cgroup.starts_with('/') || cgroup == "/" || cgroup.ends_with('/') {
                return Err(message("invalid supervised game control group"));
            }
            s.game = Some(TrackedGame { cgroup, primary: None });
            s.restore_library = false;
            Ok(())
        }).await
    }
    pub async fn prepare_menu(&self, game_active: bool) -> Result<()> {
        self.run(move |s| s.prepare_menu(game_active)).await
    }
    pub async fn focus_menu(&self) -> Result<()> { self.run(|s| s.focus_menu()).await }
    pub async fn finish_menu(&self, game_active: bool) -> Result<()> {
        self.run(move |s| s.finish_menu(game_active)).await
    }
    pub async fn game_ended(&self) -> Result<()> { self.run(State::game_ended).await }
    pub async fn reconcile(&self, menu_open: bool, game_active: bool) -> Result<()> {
        self.run(move |s| s.reconcile(menu_open, game_active)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Debug)]
    struct Fake { tree: Mutex<serde_json::Value>, commands: Mutex<Vec<String>>, fail: AtomicBool }
    impl Backend for Fake {
        fn tree(&self) -> Result<Node> { Ok(serde_json::from_value(self.tree.lock().unwrap().clone())?) }
        fn command(&self, command: &str) -> Result<()> {
            self.commands.lock().unwrap().push(command.into());
            if self.fail.swap(false, Ordering::SeqCst) { Err(message("IPC interrupted")) } else { Ok(()) }
        }
        fn owns_pid(&self, pid: u32, _: &str) -> bool { pid == 200 || pid == 201 }
    }
    fn fixture(mode: u8) -> (State, Arc<Fake>) {
        let fake = Arc::new(Fake { tree: Mutex::new(json!({"nodes": [
            {"id":2,"type":"workspace", "name":"library", "fullscreen_mode":1,
             "nodes":[{"type":"con","id":10,"pid":100,"app_id":PEGASUS}]},
            {"id":3,"type":"workspace", "name":"game", "fullscreen_mode":1,
             "nodes":[{"type":"con","id":20,"pid":200,"focused":true,"fullscreen_mode":mode}],
             "floating_nodes":[{"type":"floating_con","id":30,"pid":300,"app_id":MENU}]}
        ]})), commands: Mutex::new(vec![]), fail: AtomicBool::new(false) });
        let state = State { backend: Some(fake.clone()),
            game: Some(TrackedGame { cgroup: "/game".into(), primary: Some(20) }), lease: None,
            restore_library: false, stores: BTreeMap::new(), active_store: None, restore_store_focus: false };
        (state, fake)
    }
    #[test]
    fn steam_game_identity_excludes_client_and_malformed_classes() {
        assert!(steam_game_class("steam_app_1234"));
        for class in ["Steam", "steamwebhelper", "steam_app_0", "steam_app_bad", "steam_app_", "steam_app_123;kill"] {
            assert!(!steam_game_class(class));
        }
        assert!(steam_game_environment(b"PATH=/bin\0SteamAppId=1234\0"));
        assert!(!steam_game_environment(b"SteamAppId=0\0"));
        assert!(!steam_game_environment(b"OtherSteamAppId=1234\0"));
    }
    #[test]
    fn store_windows_move_out_of_library_and_repair_fullscreen() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.stores.insert("dukkan".into(), TrackedGame { cgroup: "/store".into(), primary: None });
        state.activate_store(Some("dukkan".into())).unwrap();
        let commands = fake.commands.lock().unwrap();
        assert!(commands.iter().any(|c| c == "[con_id=20] move container to workspace \"store-dukkan\""));
        assert!(commands.iter().any(|c| c == "[con_id=20] fullscreen enable"));
        assert!(!commands.iter().any(|c| c == "[con_id=10] fullscreen enable"));
    }
    #[test]
    fn steam_workspace_is_selected_even_before_a_client_maps() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.activate_store(Some("steam".into())).unwrap();
        assert_eq!(fake.commands.lock().unwrap().as_slice(),
            &["workspace --no-auto-back-and-forth \"store-steam\"".to_string()]);
        assert_eq!(state.active_store.as_deref(), Some("steam"));
    }
    #[test]
    fn failed_workspace_selection_preserves_previous_store_activation() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.active_store = Some("dukkan".into());
        fake.fail.store(true, Ordering::SeqCst);
        assert!(state.activate_store(Some("steam".into())).is_err());
        assert_eq!(state.active_store.as_deref(), Some("dukkan"));
        assert!(!state.restore_store_focus);
    }
    #[test]
    fn steam_client_stays_tiled_when_routed_out_of_library() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.stores.insert("steam".into(), TrackedGame { cgroup: "/store".into(), primary: None });
        state.activate_store(Some("steam".into())).unwrap();
        let commands = fake.commands.lock().unwrap();
        assert_eq!(commands.first().map(String::as_str),
            Some("workspace --no-auto-back-and-forth \"store-steam\""));
        assert!(commands.contains(&"[con_id=20] move container to workspace \"store-steam\"".into()));
        assert!(commands.contains(&"[con_id=20] focus".into()));
        assert!(!commands.iter().any(|c| c.contains("fullscreen")));
    }
    #[test]
    fn steam_game_fullscreen_is_preserved_and_client_restored_after_exit() {
        let (mut state, fake) = fixture(1);
        state.game = None;
        state.stores.insert("steam".into(), TrackedGame { cgroup: "/store".into(), primary: None });
        {
            let mut tree = fake.tree.lock().unwrap();
            tree["nodes"][1]["name"] = json!("store-steam");
            tree["nodes"][1]["nodes"][0]["window_properties"] = json!({"class":"steam_app_123"});
            tree["nodes"][1]["nodes"].as_array_mut().unwrap().push(json!({
                "type":"con", "id":21, "pid":201, "focused":false,
                "border":"none", "fullscreen_mode":0,
                "window_properties":{"class":"steam"}
            }));
        }
        state.activate_store(Some("steam".into())).unwrap();
        assert_eq!(state.stores["steam"].primary, Some(20));
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c.contains("fullscreen")));
        {
            let mut tree = fake.tree.lock().unwrap();
            tree["nodes"][1]["nodes"].as_array_mut().unwrap().remove(0);
            tree["nodes"][1]["nodes"][0]["focused"] = json!(true);
        }
        fake.commands.lock().unwrap().clear();
        state.route_stores().unwrap();
        assert_eq!(state.stores["steam"].primary, Some(21));
        let commands = fake.commands.lock().unwrap();
        assert!(commands.contains(&"[con_id=21] focus".into()));
        assert!(!commands.iter().any(|c| c.contains("fullscreen")));
    }
    #[test]
    fn newly_focused_client_replaces_splash_but_floating_popup_does_not() {
        for floating in [false, true] {
            let (mut state, fake) = fixture(0);
            state.game = None;
            state.stores.insert("dukkan".into(), TrackedGame { cgroup: "/store".into(), primary: Some(20) });
            let mut tree = fake.tree.lock().unwrap();
            tree["nodes"][1]["nodes"][0]["focused"] = json!(false);
            let key = if floating { "floating_nodes" } else { "nodes" };
            tree["nodes"][1][key].as_array_mut().unwrap().push(json!({"id":21,"pid":201,"focused":true,"border":"none"}));
            drop(tree);
            state.route_stores().unwrap();
            assert_eq!(state.stores["dukkan"].primary, Some(if floating { 20 } else { 21 }));
        }
    }
    #[test]
    fn failed_store_focus_is_retried_until_a_window_is_focused() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.stores.insert("dukkan".into(), TrackedGame { cgroup: "/store".into(), primary: Some(20) });
        state.active_store = Some("dukkan".into());
        state.restore_store_focus = true;
        fake.fail.store(true, Ordering::SeqCst);
        assert!(state.route_stores().is_err());
        assert!(state.restore_store_focus);
        state.route_stores().unwrap();
        assert!(!state.restore_store_focus);
        assert!(fake.commands.lock().unwrap().iter().any(|c| c == "[con_id=20] focus"));
    }
    #[test]
    fn native_window_properties_can_be_null() {
        let node: Node = serde_json::from_str(r#"{"id":1,"pid":100,"window_properties":null}"#).unwrap();
        assert!(!node.views()[0].steam_game);
    }
    #[test]
    fn descendants_match_but_similarly_named_or_unrelated_groups_do_not() {
        assert!(in_cgroup("0::/game/child\n", "/game"));
        assert!(in_cgroup("1:name=systemd:/game\n", "/game"));
        assert!(!in_cgroup("0::/game-other\n", "/game"));
        assert!(!in_cgroup("1:memory:/game\n", "/game"));
        assert!(!in_cgroup("0::/game\n", "/"));
    }
    #[test]
    fn restore_keeps_global_fullscreen_and_hides_menu_before_focus() {
        let (mut state, fake) = fixture(2);
        state.prepare_menu(true).unwrap();
        fake.commands.lock().unwrap().clear();
        state.finish_menu(true).unwrap();
        let commands = fake.commands.lock().unwrap();
        assert_eq!(commands[0], "[con_id=30] move scratchpad");
        assert!(commands.contains(&"[con_id=20] fullscreen enable global".into()));
        assert_eq!(commands.last().unwrap(), "[con_id=20] focus");
        assert!(state.lease.is_none());
    }
    #[test]
    fn originally_tiled_window_stays_tiled_after_menu() {
        let (mut state, fake) = fixture(0);
        state.prepare_menu(true).unwrap();
        fake.commands.lock().unwrap().clear();
        state.finish_menu(true).unwrap();
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c.contains("fullscreen enable")));
    }
    #[test]
    fn failed_apply_keeps_snapshot_for_rollback() {
        let (mut state, fake) = fixture(1);
        state.lease = Some(Lease { return_view: Some((20, Some(200))), return_game: true,
            fullscreen: BTreeMap::new() });
        fake.fail.store(true, Ordering::SeqCst);
        let view = state.views().unwrap().into_iter().find(|v| v.id == 20).unwrap();
        assert!(state.suspend(&view, None).is_err());
        assert_eq!(state.lease.as_ref().unwrap().fullscreen.len(), 1);
        state.finish_menu(true).unwrap();
        assert!(fake.commands.lock().unwrap().contains(&"[con_id=20] fullscreen enable".into()));
    }
    #[test]
    fn failed_restore_is_retryable() {
        let (mut state, fake) = fixture(1);
        state.prepare_menu(true).unwrap();
        fake.fail.store(true, Ordering::SeqCst);
        assert!(state.finish_menu(true).is_err());
        assert!(state.lease.is_some());
        state.finish_menu(true).unwrap();
        assert!(state.lease.is_none());
    }
    #[test]
    fn exiting_game_never_resumes_it_and_returns_to_library() {
        let (mut state, fake) = fixture(1);
        state.prepare_menu(true).unwrap();
        fake.commands.lock().unwrap().clear();
        state.game_ended().unwrap();
        let commands = fake.commands.lock().unwrap();
        assert!(!commands.iter().any(|c| c == "[con_id=20] focus" || c.contains("fullscreen enable")));
        assert_eq!(commands.last().unwrap(), "[con_id=10] focus");
    }
    #[test]
    fn vanished_or_replaced_game_target_is_not_restored() {
        let (mut state, fake) = fixture(1);
        state.prepare_menu(true).unwrap();
        fake.tree.lock().unwrap()["nodes"][1]["nodes"][0]["pid"] = json!(999);
        fake.commands.lock().unwrap().clear();
        state.finish_menu(true).unwrap();
        let commands = fake.commands.lock().unwrap();
        assert!(!commands.iter().any(|c| c.starts_with("[con_id=20]")));
        assert_eq!(commands.last().unwrap(), "[con_id=10] focus");
    }
    #[test]
    fn fullscreen_ancestor_is_recorded_instead_of_only_leaf() {
        let tree: Node = serde_json::from_value(json!({"id":99,"type":"workspace","fullscreen_mode":1,
            "nodes":[{"id":1,"type":"con","fullscreen_mode":1,
                "nodes":[{"id":2,"type":"con","pid":200,"focused":true}]}]})).unwrap();
        assert_eq!(tree.views()[0].fullscreen, Some((1, 1)));
    }
    #[test]
    fn workspace_metadata_never_becomes_a_fullscreen_owner() {
        let (mut state, fake) = fixture(0);
        assert!(state.views().unwrap().iter().all(|view| view.fullscreen.is_none()));
        state.prepare_menu(true).unwrap();
        assert!(state.lease.as_ref().unwrap().fullscreen.is_empty());
        state.finish_menu(true).unwrap();
        let commands = fake.commands.lock().unwrap();
        assert!(!commands.iter().any(|c| c.starts_with("[con_id=2]") || c.starts_with("[con_id=3]")));
    }
    #[test]
    fn open_menu_recovers_keyboard_focus_over_pegasus() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        fake.tree.lock().unwrap()["nodes"][0]["nodes"][0]["focused"] = json!(true);
        state.prepare_menu(false).unwrap();
        fake.commands.lock().unwrap().clear();
        state.reconcile(true, false).unwrap();
        assert_eq!(fake.commands.lock().unwrap().last().unwrap(), "[con_id=30] focus");
        fake.tree.lock().unwrap()["nodes"][1]["floating_nodes"][0]["focused"] = json!(true);
        fake.commands.lock().unwrap().clear();
        state.reconcile(true, false).unwrap();
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c == "[con_id=30] focus"));
    }
    #[test]
    fn open_menu_recovers_keyboard_focus_over_a_game() {
        let (mut state, fake) = fixture(1);
        state.prepare_menu(true).unwrap();
        fake.commands.lock().unwrap().clear();
        state.reconcile(true, true).unwrap();
        assert_eq!(fake.commands.lock().unwrap().last().unwrap(), "[con_id=30] focus");
    }
    #[test]
    fn closed_or_hidden_menu_does_not_steal_focus() {
        let (mut state, fake) = fixture(0);
        state.game = None;
        state.reconcile(false, false).unwrap();
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c == "[con_id=30] focus"));
        state.prepare_menu(false).unwrap();
        fake.tree.lock().unwrap()["nodes"][1]["name"] = json!("__i3_scratch");
        fake.commands.lock().unwrap().clear();
        state.reconcile(true, false).unwrap();
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c == "[con_id=30] focus"));
    }
    #[test]
    fn new_xwayland_child_is_routed_by_cgroup_and_unrelated_window_is_untouched() {
        let (mut state, fake) = fixture(0);
        state.game.as_mut().unwrap().primary = None;
        let mut tree = fake.tree.lock().unwrap();
        tree["nodes"][1]["nodes"] = json!([]);
        tree["nodes"][0]["nodes"].as_array_mut().unwrap().extend([
            json!({"id":21,"pid":201,"app_id":null,"focused":true,"border":"none"}),
            json!({"id":99,"pid":999,"app_id":"unrelated","border":"none"})
        ]);
        drop(tree);
        state.route_game().unwrap();
        let commands = fake.commands.lock().unwrap();
        assert!(commands.contains(&"[con_id=21] move container to workspace \"game\"".into()));
        assert!(commands.contains(&"[con_id=21] fullscreen enable, focus".into()));
        assert!(!commands.iter().any(|c| c.starts_with("[con_id=99]")));
    }
    #[test]
    fn game_first_mapped_during_menu_waits_until_close_to_become_fullscreen() {
        let (mut state, fake) = fixture(0);
        state.game.as_mut().unwrap().primary = None;
        state.lease = Some(Lease { return_view: None, return_game: true, fullscreen: BTreeMap::new() });
        state.route_game().unwrap();
        assert!(!fake.commands.lock().unwrap().iter().any(|c| c.contains("fullscreen enable")));
        assert_eq!(state.lease.as_ref().unwrap().return_view, Some((20, Some(200))));
        state.finish_menu(true).unwrap();
        assert!(fake.commands.lock().unwrap().contains(&"[con_id=20] fullscreen enable".into()));
    }
    #[test]
    fn menu_freezes_and_restores_the_library_target() {
        let (mut state, fake) = fixture(0);
        fake.tree.lock().unwrap()["nodes"][0]["nodes"][0]["focused"] = json!(true);
        fake.tree.lock().unwrap()["nodes"][1]["nodes"][0]["focused"] = json!(false);
        state.game = None;
        state.prepare_menu(false).unwrap();
        state.finish_menu(false).unwrap();
        assert_eq!(fake.commands.lock().unwrap().last().unwrap(), "[con_id=10] focus");
    }
    #[test]
    fn failed_game_exit_focus_is_retried_without_a_menu_lease() {
        let (mut state, fake) = fixture(0);
        fake.fail.store(true, Ordering::SeqCst);
        assert!(state.game_ended().is_err());
        assert!(state.restore_library);
        state.reconcile(false, false).unwrap();
        assert!(!state.restore_library);
        assert!(fake.commands.lock().unwrap().contains(&"[con_id=10] focus".into()));
    }
    #[test]
    fn no_sway_backend_leaves_legacy_sessions_supported() {
        let mut state = State { backend: None, game: None, stores: BTreeMap::new(), active_store: None, restore_store_focus: false, lease: None, restore_library: false };
        state.prepare_menu(true).unwrap();
        state.focus_menu().unwrap();
        state.finish_menu(true).unwrap();
        state.reconcile(false, false).unwrap();
        assert!(state.lease.is_none());
    }
}
