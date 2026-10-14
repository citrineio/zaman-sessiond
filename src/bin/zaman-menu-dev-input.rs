//! Manual Quickshell prototype input/focus lease. No sessiond mutation calls.
//! JSON lines on stdout/stdin are private to the development renderer.
#[allow(dead_code)]
#[path = "../error.rs"] mod error;
#[allow(dead_code)]
#[path = "../guide.rs"] mod guide;
#[allow(dead_code)]
#[path = "../inputplumber.rs"] mod inputplumber;
#[path = "zaman-menu-dev/display.rs"] mod display;
#[path = "zaman-menu-dev/display_bridge.rs"] mod display_bridge;

use error::{message, Result};
use inputplumber::SystemInputEvent;
use futures_util::StreamExt;
use tokio::task::JoinSet;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use zbus::{fdo::ObjectManagerProxy, proxy::CacheProperties, zvariant::OwnedValue, Connection, Proxy};
use zaman_sessiond::contract::{ForegroundStatusTuple, INTERFACE, PATH, SERVICE};

const INPUT_SERVICE: &str = "org.shadowblip.InputPlumber";
const COMPOSITE: &str = "org.shadowblip.Input.CompositeDevice";
const ALL: u32 = 2;
const CONTROLLER_LOST_WARNING: &str =
    "Controller disconnected or became unavailable. The menu remains open; reconnect the controller or use the keyboard.";

#[derive(Debug)]
struct ControllerLost;

impl std::fmt::Display for ControllerLost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(CONTROLLER_LOST_WARNING)
    }
}

impl std::error::Error for ControllerLost {}

// Only a typed UnknownObject means that a previously discovered object vanished.
// Do not hide access-denied, transport, timeout or unrelated method failures.
fn unknown_object(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(error) = error.downcast_ref::<zbus::Error>() {
            match error {
                zbus::Error::MethodError(name, _, _)
                    if name.as_str() == "org.freedesktop.DBus.Error.UnknownObject" => return true,
                zbus::Error::FDO(error) if unknown_object(error.as_ref()) => return true,
                _ => {},
            }
        }
        if matches!(error.downcast_ref::<zbus::fdo::Error>(),
            Some(zbus::fdo::Error::UnknownObject(_))) {
            return true;
        }
        current = error.source();
    }
    false
}

fn controller_error(error: error::DynError) -> error::DynError {
    if unknown_object(error.as_ref()) { Box::new(ControllerLost) } else { error }
}

fn warn_controller_loss(warned: &mut bool) -> Result<()> {
    warn_controller_error(warned, CONTROLLER_LOST_WARNING)
}

fn warn_controller_error(warned: &mut bool, detail: &str) -> Result<()> {
    emit(json!({"type":"input-reset"}))?;
    if !*warned {
        timing_log(&format!("[menu-input] {detail}"));
        emit(json!({"type":"warning", "message":format!("{CONTROLLER_LOST_WARNING} ({detail})")}))?;
        *warned = true;
    }
    Ok(())
}

fn emit(value: Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}

async fn foreground(connection: &Connection) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(1), async {
        let proxy = Proxy::new(connection, SERVICE, PATH, INTERFACE).await?;
        let status: ForegroundStatusTuple = proxy.call("ForegroundStatus", &()).await?;
        Ok::<_, error::DynError>(status.0)
    }).await?
}

fn owns_foreground(foreground: &str) -> bool {
    matches!(foreground, "library" | "game" | "storefront")
}

#[derive(Clone, Debug, PartialEq)]
struct ControllerInfo {
    path: String,
    name: String,
    profile: String,
    sources: Vec<String>,
    targets: Vec<String>,
    dbus: Vec<String>,
    mode: u32,
}

impl ControllerInfo {
    fn same_source(&self, other: &Self) -> bool {
        self.path == other.path && self.name == other.name && self.sources == other.sources
    }
    fn view(&self) -> Value {
        json!({"id": self.path, "name": self.name, "profile": self.profile,
            "targets": self.targets, "menuInput": !self.dbus.is_empty()})
    }
}

#[derive(Clone)]
struct ControllerSnapshot {
    connection: Connection,
    owner: String,
    devices: BTreeMap<String, ControllerInfo>,
}

fn snapshot_property<T>(properties: &HashMap<String, OwnedValue>, name: &str) -> Result<T>
where T: TryFrom<OwnedValue>, T::Error: std::error::Error + Send + Sync + 'static {
    let value = properties.get(name)
        .ok_or_else(|| message(format!("controller snapshot missing property {name}")))?;
    Ok(T::try_from(value.try_clone()?)?)
}

fn controller_info(path: &str, properties: &HashMap<String, OwnedValue>) -> Result<ControllerInfo> {
    let mut sources: Vec<String> = snapshot_property(properties, "SourceDevicePaths")?;
    let mut targets: Vec<String> = snapshot_property(properties, "TargetDevices")?;
    let mut dbus: Vec<String> = snapshot_property(properties, "DbusDevices")?;
    sources.sort(); targets.sort(); dbus.sort();
    Ok(ControllerInfo { path: path.to_owned(),
        name: snapshot_property(properties, "Name")?,
        profile: snapshot_property(properties, "ProfileName")?,
        sources, targets, dbus, mode: snapshot_property(properties, "InterceptMode")? })
}

// Identity/mode reads must be fresh, and must not trigger GetAll for unrelated
// capabilities/profile/filter properties on every short-lived proxy.
async fn live_composite<'a>(connection: &'a Connection, owner: &'a str, path: &'a str) -> zbus::Result<Proxy<'a>> {
    zbus::proxy::Builder::<Proxy<'a>>::new(connection)
        .destination(owner)?.path(path)?.interface(COMPOSITE)?
        .cache_properties(CacheProperties::No).build().await
}

async fn controller_snapshot(connection: &Connection) -> Result<ControllerSnapshot> {
    controller_snapshot_inner(connection).await.map_err(controller_error)
}

async fn controller_snapshot_inner(connection: &Connection) -> Result<ControllerSnapshot> {
    let bus = Proxy::new(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
    let owner: String = bus.call("GetNameOwner", &(INPUT_SERVICE,)).await?;
    // Pin all property reads to this unique owner, not a replacement daemon.
    let manager = ObjectManagerProxy::builder(connection)
        .destination(owner.as_str())?.path("/org/shadowblip/InputPlumber")?.build().await?;
    let objects = manager.get_managed_objects().await?;
    let mut devices = BTreeMap::new();
    // ObjectManager already includes these properties. Do not rediscover on a
    // second connection or populate another property cache per controller.
    for (path, interfaces) in objects {
        let Some((_, properties)) = interfaces.iter().find(|(name, _)| name.as_str() == COMPOSITE) else { continue; };
        let info = controller_info(path.as_str(), properties)?;
        devices.insert(info.path.clone(), info);
    }
    if bus.call::<_, _, String>("GetNameOwner", &(INPUT_SERVICE,)).await? != owner {
        return Err(message("controller service changed during discovery"));
    }
    Ok(ControllerSnapshot { connection: connection.clone(), owner, devices })
}

// Controller failures are returned separately from foreground failures. Only
// the former are recoverable; a controller error must never exit the UI loop.
async fn next_controller_snapshot(connection: Option<Connection>, session: Connection, reconnect: bool) -> Result<(Result<ControllerSnapshot>, String)> {
    tokio::time::sleep(Duration::from_secs(2)).await;
    let snapshot = async {
        let connection = match connection {
            Some(connection) if !reconnect => connection,
            _ => Connection::system().await?,
        };
        tokio::time::timeout(Duration::from_secs(3), controller_snapshot(&connection)).await?
    }.await;
    let current_foreground = foreground(&session).await?;
    Ok((snapshot, current_foreground))
}

// A matching ALL mode is ours only for a source we actually intercepted.
fn may_refresh(previous: Option<&ControllerInfo>, touched: bool, current: &ControllerInfo) -> bool {
    if touched {
        previous.is_some_and(|old| old.same_source(current)) && current.mode == ALL
    } else {
        // A replaced source whose old lease was discarded is a new device.
        // Its current mode must still prove that another owner has not taken it.
        current.mode != ALL
    }
}

fn lost_controller(
    previous: &BTreeMap<String, ControllerInfo>,
    touched: &BTreeSet<String>,
    current: &BTreeMap<String, ControllerInfo>,
) -> bool {
    touched.iter().any(|path| previous.get(path).is_some_and(|old|
        !old.dbus.is_empty() && current.get(path).is_none_or(|new|
            !old.same_source(new) || new.dbus.is_empty())))
}

fn retain_restoration_rights(saved: &mut Vec<(String, u32)>, touched: &mut BTreeSet<String>,
    previous: &BTreeMap<String, ControllerInfo>, current: &BTreeMap<String, ControllerInfo>, owner_changed: bool) {
    saved.retain(|(path, _)| !owner_changed && current.get(path).is_some_and(|new|
        may_refresh(previous.get(path), touched.contains(path), new)));
    touched.retain(|path| saved.iter().any(|(saved, _)| saved == path));
}

struct ControllerLease {
    connection: Option<Connection>,
    owner: String,
    saved: Vec<(String, u32)>,
    touched: BTreeSet<String>,
    devices: BTreeMap<String, ControllerInfo>,
}

impl ControllerLease {
    fn from_snapshot(snapshot: ControllerSnapshot) -> Self {
        Self { connection: Some(snapshot.connection), owner: snapshot.owner,
            saved: Vec::new(), touched: BTreeSet::new(), devices: snapshot.devices }
    }
    fn snapshot(&self) -> Option<ControllerSnapshot> {
        Some(ControllerSnapshot { connection: self.connection.clone()?,
            owner: self.owner.clone(), devices: self.devices.clone() })
    }
    async fn discover() -> (Self, Option<String>) {
        // Even an unavailable system bus must not prevent keyboard-only startup.
        let snapshot = tokio::time::timeout(Duration::from_secs(2), async {
            let connection = Connection::system().await?;
            controller_snapshot(&connection).await
        }).await;
        match snapshot {
            Ok(Ok(snapshot)) => (Self::from_snapshot(snapshot), None),
            result => {
                let detail = match result {
                    Ok(Err(error)) => error.to_string(),
                    Err(error) => error.to_string(),
                    Ok(Ok(_)) => unreachable!(),
                };
                (Self { connection: None, owner: String::new(), saved: Vec::new(),
                    touched: BTreeSet::new(), devices: BTreeMap::new() }, Some(detail))
            }
        }
    }
    async fn acquire(&mut self) -> Result<bool> {
        let Some(connection) = self.connection.as_ref() else { return Ok(true); };
        let mut unavailable = false;
        for (path, current) in &self.devices {
            if current.dbus.is_empty() || self.touched.contains(path) { continue; }
            let check_started = Instant::now();
            let proxy = live_composite(connection, self.owner.as_str(), path.as_str()).await?;
            // These read-only identity checks are independent. Overlap their
            // round trips; all must finish before reading ownership or writing.
            let (mut sources, name, mut dbus) = tokio::try_join!(
                proxy.get_property::<Vec<String>>("SourceDevicePaths"),
                proxy.get_property::<String>("Name"),
                proxy.get_property::<Vec<String>>("DbusDevices"),
            )?;
            sources.sort();
            dbus.sort();
            if sources != current.sources || name != current.name || dbus != current.dbus {
                return Err(Box::new(ControllerLost));
            }
            timing_log(&format!("[menu-acquire] {path} live-identity: {} ms", check_started.elapsed().as_millis()));
            // Keep the ownership read immediately before deciding to acquire.
            let mode_started = Instant::now();
            let mode: u32 = proxy.get_property("InterceptMode").await?;
            timing_log(&format!("[menu-acquire] {path} mode-read: {} ms", mode_started.elapsed().as_millis()));
            if mode == ALL {
                // Unknown ALL ownership is never stolen, including startup races.
                unavailable = true;
                continue;
            }
            if let Some((_, original)) = self.saved.iter_mut().find(|(saved, _)| saved == path) {
                *original = mode;
            } else { self.saved.push((path.clone(), mode)); }
            // Record before IPC: a cancelled write may already have applied.
            self.touched.insert(path.clone());
            let intercept_started = Instant::now();
            proxy.set_property("InterceptMode", ALL).await?;
            timing_log(&format!("[menu-acquire] {path} intercept-write: {} ms", intercept_started.elapsed().as_millis()));
        }
        Ok(unavailable && !self.devices.values().any(|device|
            self.touched.contains(&device.path) && !device.dbus.is_empty()))
    }
    async fn refresh(&mut self, snapshot: ControllerSnapshot) -> Result<bool> {
        let owner_changed = snapshot.owner != self.owner;
        let lost = owner_changed || lost_controller(&self.devices, &self.touched, &snapshot.devices);
        let ownership_changed = self.touched.iter().any(|path| snapshot.devices.get(path)
            .is_some_and(|current| !may_refresh(self.devices.get(path), true, current)));
        // Discard old restoration rights on service restart, identity replacement
        // or released interception. Never restore an old mode onto a new owner.
        retain_restoration_rights(&mut self.saved, &mut self.touched, &self.devices,
            &snapshot.devices, owner_changed);
        self.owner = snapshot.owner;
        self.connection = Some(snapshot.connection);
        self.devices = snapshot.devices;
        let unavailable = self.acquire().await?;
        Ok(lost || ownership_changed || unavailable)
    }
    fn monitor(&self) -> Option<ControllerMonitor> {
        let targets = self.devices.values().filter(|device| self.touched.contains(&device.path))
            .flat_map(|device| device.dbus.iter().cloned()).collect::<BTreeSet<_>>();
        ControllerMonitor::new(self.connection.as_ref()?, &self.owner, targets)
    }
    fn views(&self) -> Vec<Value> {
        self.devices.values().map(|device| {
            let mut view = device.view();
            view["menuInput"] = json!(self.touched.contains(&device.path) && !device.dbus.is_empty());
            view
        }).collect()
    }
    async fn restore(&self, current_foreground: Option<&str>) -> Result<()> {
        // The production menu/Transfer may have taken ownership in the interim.
        if current_foreground.is_some_and(|f| !owns_foreground(f)) { return Ok(()); }
        let Some(connection) = self.connection.as_ref() else { return Ok(()); };
        let bus = Proxy::new(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
        let owner: String = bus.call("GetNameOwner", &(INPUT_SERVICE,)).await?;
        if owner != self.owner { return Ok(()); }
        let mut failures = Vec::new();
        for (path, original) in &self.saved {
            if !self.touched.contains(path) { continue; }
            let result = tokio::time::timeout(Duration::from_millis(750), async {
                let proxy = live_composite(connection, self.owner.as_str(), path.as_str()).await?;
                let mut sources: Vec<String> = proxy.get_property("SourceDevicePaths").await?;
                sources.sort();
                let name: String = proxy.get_property("Name").await?;
                if self.devices.get(path).is_none_or(|old| old.sources != sources || old.name != name) {
                    return Ok::<_, error::DynError>(());
                }
                let current: u32 = proxy.get_property("InterceptMode").await?;
                // A different owner or daemon refresh may already have restored it.
                if current == ALL { proxy.set_property("InterceptMode", *original).await?; }
                Ok::<_, error::DynError>(())
            }).await;
            match result {
                Ok(Ok(())) => {},
                // Disconnection can race any property read or the final write.
                // There is no live object left to restore; preserve other errors.
                Ok(Err(error)) if unknown_object(error.as_ref()) => {},
                Ok(Err(error)) => failures.push(format!("{path}: {error}")),
                Err(error) => failures.push(format!("{path}: {error}")),
            }
        }
        if failures.is_empty() { Ok(()) } else { Err(message(failures.join("; "))) }
    }
}

#[derive(Clone, Debug)]
struct Window { id: u64, pid: u64, app_id: String, focused: bool }

fn windows(tree: &Value) -> Vec<Window> {
    fn walk(node: &Value, out: &mut Vec<Window>) {
        if let (Some(id), Some(pid)) = (node["id"].as_u64(), node["pid"].as_u64()) {
            out.push(Window { id, pid, app_id: node["app_id"].as_str().unwrap_or("").into(),
                focused: node["focused"].as_bool().unwrap_or(false) });
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(nodes) = node[key].as_array() { for child in nodes { walk(child, out); } }
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut out);
    out
}

fn restore_target(tree: &Value, saved: Option<&Window>, initial: &str, current: &str) -> Option<u64> {
    let views = windows(tree);
    if initial == current {
        if let Some(saved) = saved {
            if views.iter().any(|v| v.id == saved.id && v.pid == saved.pid) { return Some(saved.id); }
        }
    }
    if current == "game" || current == "storefront" {
        views.iter().find(|v| v.focused).map(|v| v.id)
    } else {
        views.iter().find(|v| v.app_id == "org.pegasus-frontend.pegasus-fe").map(|v| v.id)
    }
}

fn sway_request(socket: &PathBuf, kind: u32, payload: &[u8]) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)?;
    let timeout = Some(Duration::from_millis(750));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    stream.write_all(b"i3-ipc")?;
    stream.write_all(&(payload.len() as u32).to_ne_bytes())?;
    stream.write_all(&kind.to_ne_bytes())?;
    stream.write_all(payload)?;
    let mut header = [0_u8; 14];
    stream.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header[6..10].try_into()?) as usize;
    if &header[..6] != b"i3-ipc" || u32::from_ne_bytes(header[10..14].try_into()?) != kind || size > 8 * 1024 * 1024 {
        return Err(message("invalid Sway IPC response"));
    }
    let mut body = vec![0; size];
    stream.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

async fn tree(socket: &PathBuf) -> Result<Value> {
    let socket = socket.clone();
    tokio::task::spawn_blocking(move || sway_request(&socket, 4, b"")).await?
}

async fn restore_focus(socket: &PathBuf, saved: Option<Window>, initial: String, current: String) -> Result<()> {
    let socket = socket.clone();
    tokio::task::spawn_blocking(move || {
        let tree = sway_request(&socket, 4, b"")?;
        let command = match restore_target(&tree, saved.as_ref(), &initial, &current) {
            Some(id) => format!("[con_id={id}] focus"),
            None if current == "library" => "workspace \"library\"".into(),
            None => return Ok(()),
        };
        let replies = sway_request(&socket, 0, command.as_bytes())?;
        let replies = replies.as_array().ok_or_else(|| message("invalid Sway command reply"))?;
        if replies.is_empty() { return Err(message("empty Sway command reply")); }
        for reply in replies {
            if reply["success"].as_bool() != Some(true) {
                return Err(message(format!("Sway command {command:?}: {}", reply["error"])));
            }
        }
        Ok(())
    }).await?
}

// Listeners are pinned to the daemon generation and only to controllers
// acquired by this preview. One failed listener must be visible even when
// another controller keeps its channel alive.
struct ControllerMonitor {
    receiver: mpsc::Receiver<Result<SystemInputEvent>>,
    listeners: JoinSet<()>,
}
impl ControllerMonitor {
    fn new(connection: &Connection, owner: &str, targets: BTreeSet<String>) -> Option<Self> {
        if targets.is_empty() { return None; }
        let (sender, receiver) = mpsc::channel(64);
        let mut listeners = JoinSet::new();
        for target in targets {
            let connection = connection.clone();
            let owner = owner.to_owned();
            let sender = sender.clone();
            listeners.spawn(async move {
                let result: Result<()> = async {
                    let proxy = Proxy::new(&connection, owner.as_str(), target.as_str(),
                        "org.shadowblip.Input.DBusDevice").await?;
                    let mut signals = proxy.receive_signal("InputEvent").await?;
                    while let Some(signal) = signals.next().await {
                        let (event, value): (String, f64) = signal.body().deserialize()?;
                        if menu_event(&event) && value.is_finite() {
                            if sender.send(Ok(SystemInputEvent::MenuInput { event, value })).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Err(message(format!("controller listener ended: {target}")))
                }.await;
                if let Err(error) = result { let _ = sender.send(Err(error)).await; }
            });
        }
        drop(sender);
        Some(Self { receiver, listeners })
    }
}
impl Drop for ControllerMonitor {
    fn drop(&mut self) { self.listeners.abort_all(); }
}
async fn next_input(monitor: &mut Option<ControllerMonitor>) -> Option<Result<SystemInputEvent>> {
    match monitor { Some(monitor) => monitor.receiver.recv().await, None => std::future::pending().await }
}

fn menu_event(event: &str) -> bool {
    matches!(event, "ui_up"|"ui_down"|"ui_left"|"ui_right"|"ui_accept"|"ui_cancel"|"ui_back"|"ui_context"|"ui_action")
}

fn usable_press(event: &str, value: f64, presented: bool) -> bool {
    presented && menu_event(event) && value.is_finite() && value > 0.5
}

fn forward_input(event: SystemInputEvent, presented: bool, pressed: &mut BTreeSet<String>, warned: &mut bool) -> Result<()> {
    // Guide stays with the production fallback, even during hotplug setup.
    if let SystemInputEvent::MenuInput { event, value } = event {
        if menu_event(&event) && value.is_finite() {
            let changed = if value > 0.5 { pressed.insert(event.clone()) } else { pressed.remove(&event) };
            // Discovery and constructing a monitor do not prove its subscriptions
            // work. Clear the loss episode only after a usable controller press.
            if *warned && changed && usable_press(&event, value, presented) {
                emit(json!({"type":"controller-recovered"}))?;
                *warned = false;
            }
            if presented && changed { emit(json!({"type":"input","event":event,"value":value}))?; }
        }
    }
    Ok(())
}

fn startup_timing(started: Instant, previous: &mut Instant, phase: &str) {
    let now = Instant::now();
    timing_log(&format!("[menu-startup] {phase}: {} ms (total {} ms)",
        now.duration_since(*previous).as_millis(), now.duration_since(started).as_millis()));
    *previous = now;
}

fn timing_log(line: &str) {
    eprintln!("{line}");
    // Direct diagnostics must work even when the user journal has no records.
    // Runtime directory belongs to this user; diagnostics are append-only.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime).join("zaman-menu-timing.log");
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
            let _ = writeln!(log, "pid={} {line}", std::process::id());
        }
    }
}

fn stdin_commands() -> mpsc::Receiver<String> {
    let (sender, commands) = mpsc::channel(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.blocking_send(line).is_err() { break; }
        }
    });
    commands
}

async fn idle_snapshot(connection: Option<Connection>, delay: Duration) -> Result<ControllerSnapshot> {
    tokio::time::sleep(delay).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        let connection = match connection {
            Some(connection) => connection,
            None => Connection::system().await?,
        };
        controller_snapshot(&connection).await
    }).await?
}

fn watch_idle_inventory(sender: mpsc::Sender<()>) -> JoinSet<()> {
    let mut listeners = JoinSet::new();
    listeners.spawn(async move {
        while !sender.is_closed() {
            let result: Result<()> = async {
                let connection = Connection::system().await?;
                let manager = Proxy::new(&connection, INPUT_SERVICE,
                    "/org/shadowblip/InputPlumber", "org.freedesktop.DBus.ObjectManager").await?;
                let mut added = manager.receive_signal("InterfacesAdded").await?;
                let mut removed = manager.receive_signal("InterfacesRemoved").await?;
                let bus = Proxy::new(&connection, "org.freedesktop.DBus",
                    "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
                let mut owners = bus.receive_signal("NameOwnerChanged").await?;
                // Refresh once subscriptions are live, closing the setup gap.
                let _ = sender.try_send(());
                loop {
                    tokio::select! {
                        signal = added.next() => {
                            if signal.is_none() { return Err(message("inventory added listener ended")); }
                            let _ = sender.try_send(());
                        }
                        signal = removed.next() => {
                            if signal.is_none() { return Err(message("inventory removed listener ended")); }
                            let _ = sender.try_send(());
                        }
                        signal = owners.next() => {
                            let Some(signal) = signal else { return Err(message("inventory owner listener ended")); };
                            let (name, _, _): (String, String, String) = signal.body().deserialize()?;
                            if name == INPUT_SERVICE { let _ = sender.try_send(()); }
                        }
                    }
                }
            }.await;
            // A broken subscription makes any cached inventory untrustworthy.
            if result.is_err() { let _ = sender.try_send(()); }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    listeners
}

async fn run(resident: bool) -> Result<()> {
    // Separate idle singleton: the per-session lock below still signals an
    // active preview to the shortcut listener, and prevents overlapping leases.
    let _resident_lock = if resident {
        let runtime = std::env::var("XDG_RUNTIME_DIR")?;
        let address = SocketAddr::from_abstract_name(format!("zaman-menu-resident:{runtime}").as_bytes())?;
        Some(UnixListener::bind_addr(&address)?)
    } else { None };
    let session = Connection::session().await?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut commands = stdin_commands();
    let mut shutdown = false;
    if !resident {
        run_session(&session, &mut commands, &mut term, &mut interrupt, &mut shutdown, None).await?;
        return Ok(());
    }

    emit(json!({"type":"helper-idle", "protocol":1}))?;
    let (changes, mut inventory_changes) = mpsc::channel(1);
    let _inventory_listener = watch_idle_inventory(changes);
    let mut inventory_watch_alive = true;
    let mut cache: Option<ControllerSnapshot> = None;
    let mut cache_updated = Instant::now();
    let mut scan = Box::pin(idle_snapshot(None, Duration::ZERO));
    loop {
        tokio::select! {
            // Device/service changes invalidate cache before a simultaneous open.
            biased;
            change = inventory_changes.recv(), if inventory_watch_alive => {
                if change.is_none() { inventory_watch_alive = false; }
                let connection = cache.as_ref().map(|snapshot| snapshot.connection.clone());
                cache = None;
                scan = Box::pin(idle_snapshot(connection, Duration::ZERO));
            }
            result = &mut scan => {
                cache = result.ok();
                cache_updated = Instant::now();
                let connection = cache.as_ref().map(|snapshot| snapshot.connection.clone());
                scan = Box::pin(idle_snapshot(connection, Duration::from_secs(15)));
            }
            _ = term.recv() => break,
            _ = interrupt.recv() => break,
            command = commands.recv() => {
                let Some(command) = command else { break };
                let Ok(command) = serde_json::from_str::<Value>(&command) else { continue };
                if command["action"].as_str() != Some("open") { continue; }
                // Never rely on unbounded old metadata. Every cached lease is
                // owner-checked and source/name/target/mode-checked at acquisition.
                let preloaded = if cache_updated.elapsed() <= Duration::from_secs(20) { cache.take() } else { None };
                match run_session(&session, &mut commands, &mut term, &mut interrupt, &mut shutdown, preloaded).await {
                    Ok(snapshot) => cache = snapshot,
                    Err(error) => {
                        timing_log(&format!("[menu-helper] session error: {error}"));
                        emit(json!({"type":"error", "message":error.to_string()}))?;
                        cache = None;
                    }
                }
                if shutdown { break; }
                cache_updated = Instant::now();
                let connection = cache.as_ref().map(|snapshot| snapshot.connection.clone());
                // Restoration completed before declaring idle; a new lease may
                // now start without restarting the helper or the session bus.
                emit(json!({"type":"helper-idle", "protocol":1}))?;
                // The just-restored lease already supplies recent metadata;
                // avoid a broad scan competing with an immediate next opening.
                let delay = if cache.is_some() { Duration::from_secs(15) } else { Duration::ZERO };
                scan = Box::pin(idle_snapshot(connection, delay));
            }
        }
    }
    Ok(())
}

async fn run_session(session: &Connection, commands: &mut mpsc::Receiver<String>,
    term: &mut tokio::signal::unix::Signal, interrupt: &mut tokio::signal::unix::Signal,
    shutdown: &mut bool, preloaded: Option<ControllerSnapshot>) -> Result<Option<ControllerSnapshot>> {
    let startup_started = Instant::now();
    let mut phase_started = startup_started;
    timing_log("[menu-startup] begin");
    let runtime = std::env::var("XDG_RUNTIME_DIR")?;
    // Abstract Unix address releases automatically, including on process death.
    let lock_name = format!("zaman-menu-dev:{runtime}");
    let address = SocketAddr::from_abstract_name(lock_name.as_bytes())?;
    let _lock = UnixListener::bind_addr(&address)
        .map_err(|error| message(format!("development preview already running or lock unavailable: {error}")))?;
    let socket = PathBuf::from(std::env::var_os("SWAYSOCK").ok_or_else(|| message("SWAYSOCK is not set"))?);
    let initial = foreground(session).await?;
    if !owns_foreground(&initial) { return Err(message("close the production menu/Transfer before launching this preview")); }
    startup_timing(startup_started, &mut phase_started, "session/foreground");
    let saved = windows(&tree(&socket).await?).into_iter().find(|v| v.focused);
    startup_timing(startup_started, &mut phase_started, "save-focus");
    let preloaded = if let Some(snapshot) = preloaded {
        let valid = tokio::time::timeout(Duration::from_millis(500), async {
            let bus = Proxy::new(&snapshot.connection, "org.freedesktop.DBus",
                "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
            let owner: String = bus.call("GetNameOwner", &(INPUT_SERVICE,)).await?;
            Ok::<_, error::DynError>(owner == snapshot.owner)
        }).await;
        if matches!(valid, Ok(Ok(true))) { Some(snapshot) } else { None }
    } else { None };
    let reused = preloaded.is_some();
    let (mut lease, startup_warning) = match preloaded {
        Some(snapshot) => (ControllerLease::from_snapshot(snapshot), None),
        None => ControllerLease::discover().await,
    };
    startup_timing(startup_started, &mut phase_started,
        if reused { "cached-controller-snapshot" } else { "controller-discovery" });
    let mut monitor = None;
    let display = display_bridge::DisplayBridge::new().await?;
    startup_timing(startup_started, &mut phase_started, "display-worker");

    let mut pressed = BTreeSet::new();
    let mut input_warning = false;
    let operation = async {
        let mut startup_detail = startup_warning;
        match tokio::time::timeout(Duration::from_secs(2), lease.acquire()).await {
            Ok(Ok(false)) => {},
            Ok(Ok(true)) if startup_detail.is_none() => startup_detail = Some("controller already intercepted by another owner".into()),
            Ok(Ok(true)) => {},
            Ok(Err(error)) => startup_detail = Some(error.to_string()),
            Err(error) => startup_detail = Some(error.to_string()),
        }
        startup_timing(startup_started, &mut phase_started, "controller-acquire");
        let startup_lost = startup_detail.is_some();
        if !startup_lost { monitor = lease.monitor(); }
        // Recheck after asynchronous setup, before showing an exclusive layer.
        if foreground(&session).await? != initial { return Err(message("foreground changed during preview startup")); }
        startup_timing(startup_started, &mut phase_started, "foreground-recheck");
        let proxy = Proxy::new(session, SERVICE, PATH, INTERFACE).await?;
        let store: (String, u64, String, String, bool, String) = proxy.call("StorefrontStatus", &()).await?;
        emit(json!({"type":"ready", "foreground":initial,
            "storefront":store.0, "storeGeneration":store.1.to_string(), "storeGame":store.4, "guideOwner":store.3,
            "display":true,
            "controllers":if startup_lost { 0 } else { lease.devices.values().filter(|device| lease.touched.contains(&device.path) && !device.dbus.is_empty()).count() },
            "devices":if startup_lost { Vec::new() } else { lease.views() }}))?;
        startup_timing(startup_started, &mut phase_started, "ready");
        if startup_lost {
            monitor = None;
            warn_controller_error(&mut input_warning, startup_detail.as_deref().unwrap())?;
        }
        let mut presented = false;
        let mut last_heartbeat = Instant::now();
        let started = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Keep these futures alive across input events. Awaiting D-Bus inside
        // a selected tick branch prevented the loop from reading input until
        // the entire discovery/property scan completed.
        let foreground_watch = async {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if foreground(&session).await? != initial {
                    return Ok::<_, error::DynError>(());
                }
            }
        };
        let mut inventory_watch = Box::pin(next_controller_snapshot(lease.connection.clone(), session.clone(), input_warning));
        tokio::pin!(foreground_watch);
        'preview: loop {
            tokio::select! {
                _ = term.recv() => { *shutdown = true; break; },
                _ = interrupt.recv() => { *shutdown = true; break; },
                result = &mut foreground_watch => { result?; break; }
                result = &mut inventory_watch => {
                    let (snapshot, current_foreground) = result?;
                    // Foreground takeover remains an explicit external close.
                    if current_foreground != initial { break; }
                    let snapshot = match snapshot {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            pressed.clear();
                            monitor = None;
                            warn_controller_error(&mut input_warning, &error.to_string())?;
                            inventory_watch = Box::pin(next_controller_snapshot(lease.connection.clone(), session.clone(), true));
                            continue;
                        }
                    };
                    let old_views = lease.views();
                    let rebuild_monitor = snapshot.owner != lease.owner
                        || snapshot.devices != lease.devices
                        || monitor.is_none();
                    if rebuild_monitor {
                        pressed.clear();
                        monitor = None;
                        emit(json!({"type":"input-reset"}))?;
                    }
                    let refreshed = {
                        let refresh = tokio::time::timeout(Duration::from_secs(2), lease.refresh(snapshot));
                        tokio::pin!(refresh);
                        // Input, close, heartbeat and foreground preemption remain
                        // responsive while new-device D-Bus writes are pending.
                        loop {
                            tokio::select! {
                                result = &mut refresh => break match result {
                                    Ok(result) => result,
                                    Err(error) => Err(error.into()),
                                },
                                _ = term.recv() => { *shutdown = true; break 'preview; },
                                _ = interrupt.recv() => { *shutdown = true; break 'preview; },
                                result = &mut foreground_watch => { result?; break 'preview; }
                                command = commands.recv() => {
                                    let Some(command) = command else { *shutdown = true; break 'preview; };
                                    if let Ok(command) = serde_json::from_str::<Value>(&command) {
                                        match command["action"].as_str() {
                                            Some("close") => break 'preview,
                                            Some("opened") => { presented = true; last_heartbeat = Instant::now(); }
                                            Some("heartbeat") => last_heartbeat = Instant::now(),
                                            _ if presented => display.handle(&command)?,
                                            _ => {},
                                        }
                                    }
                                }
                                event = next_input(&mut monitor) => {
                                    match event {
                                        Some(Ok(event)) => forward_input(event, presented, &mut pressed, &mut input_warning)?,
                                        failure => {
                                            monitor = None;
                                            pressed.clear();
                                            let detail = match failure { Some(Err(error)) => error.to_string(), _ => "controller listener closed".into() };
                                            warn_controller_error(&mut input_warning, &detail)?;
                                        }
                                    }
                                }
                            }
                        }
                    };
                    let interrupted = match refreshed {
                        Ok(interrupted) => interrupted,
                        Err(error) => {
                            pressed.clear();
                            monitor = None;
                            warn_controller_error(&mut input_warning, &error.to_string())?;
                            inventory_watch = Box::pin(next_controller_snapshot(lease.connection.clone(), session.clone(), true));
                            continue;
                        }
                    };
                    if interrupted {
                        pressed.clear();
                        monitor = None;
                        warn_controller_loss(&mut input_warning)?;
                    }
                    if rebuild_monitor || interrupted { monitor = lease.monitor(); }
                    let views = lease.views();
                    if rebuild_monitor || views != old_views {
                        emit(json!({"type":"controllers", "devices":views,
                            "controllers":lease.devices.values().filter(|device| lease.touched.contains(&device.path) && !device.dbus.is_empty()).count()}))?;
                    }
                    inventory_watch = Box::pin(next_controller_snapshot(lease.connection.clone(), session.clone(), input_warning));
                }
                command = commands.recv() => {
                    let Some(command) = command else { *shutdown = true; break; };
                    let Ok(command) = serde_json::from_str::<Value>(&command) else { continue };
                    match command["action"].as_str() {
                        Some("close") => break,
                        Some("opened") => { presented = true; last_heartbeat = Instant::now(); }
                        Some("heartbeat") => last_heartbeat = Instant::now(),
                        _ if presented => display.handle(&command)?,
                        _ => {},
                    }
                }
                event = next_input(&mut monitor) => {
                    match event {
                        Some(Ok(event)) => forward_input(event, presented, &mut pressed, &mut input_warning)?,
                        failure => {
                            monitor = None;
                            pressed.clear();
                            let detail = match failure { Some(Err(error)) => error.to_string(), _ => "controller listener closed".into() };
                            warn_controller_error(&mut input_warning, &detail)?;
                        }
                    }
                }
                _ = tick.tick() => {
                    if !presented && started.elapsed() > Duration::from_secs(3) {
                        return Err(message("preview surface did not connect within 3 seconds"));
                    }
                    if last_heartbeat.elapsed() > Duration::from_secs(5) {
                        return Err(message("preview renderer stopped responding"));
                    }
                }
            }
        }
        Ok::<_, error::DynError>(())
    }.await;

    // The renderer hides its layer before acknowledging a normal close. Also
    // explicitly request hiding on EOF, signal, preemption or startup failure.
    display.deactivate();
    let _ = emit(json!({"type":"closing"}));
    let restored_display = display.shutdown().await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    // Keep interception until the closing button is released so the D-Bus
    // target does not retain a pressed A/B/direction across the next launch.
    let release_deadline = tokio::time::sleep(Duration::from_millis(750));
    tokio::pin!(release_deadline);
    while !pressed.is_empty() {
        tokio::select! {
            _ = &mut release_deadline => break,
            event = next_input(&mut monitor) => {
                match event {
                    Some(Ok(SystemInputEvent::MenuInput { event, value })) if value <= 0.5 => { pressed.remove(&event); }
                    None => break,
                    _ => {},
                }
            }
        }
    }
    let current = foreground(&session).await.ok();
    let restored_focus = if current.as_deref().is_some_and(owns_foreground) {
        restore_focus(&socket, saved, initial, current.as_deref().unwrap().to_owned()).await
    } else { Ok(()) };
    let restored_input = lease.restore(current.as_deref()).await;
    let mut errors = Vec::new();
    for result in [operation, restored_input, restored_focus, restored_display] {
        if let Err(error) = result { errors.push(error.to_string()); }
    }
    if errors.is_empty() { emit(json!({"type":"closed"}))?; Ok(lease.snapshot()) }
    else { Err(message(errors.join("; "))) }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("--resident-version") {
        println!("zaman-menu-helper-resident-v1");
        return;
    }
    let resident = std::env::args().nth(1).as_deref() == Some("--resident");
    if let Err(error) = run(resident).await {
        timing_log(&format!("Quickshell development preview: {error}"));
        let _ = emit(json!({"type":"error","message":error.to_string()}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot_properties(device: &ControllerInfo) -> HashMap<String, OwnedValue> {
        use zbus::zvariant::Value;
        HashMap::from([
            ("Name".into(), OwnedValue::try_from(Value::from(device.name.as_str())).unwrap()),
            ("ProfileName".into(), OwnedValue::try_from(Value::from(device.profile.as_str())).unwrap()),
            ("SourceDevicePaths".into(), OwnedValue::try_from(Value::from(device.sources.clone())).unwrap()),
            ("TargetDevices".into(), OwnedValue::try_from(Value::from(device.targets.clone())).unwrap()),
            ("DbusDevices".into(), OwnedValue::try_from(Value::from(device.dbus.clone())).unwrap()),
            ("InterceptMode".into(), OwnedValue::from(device.mode)),
        ])
    }
    #[test]
    fn managed_snapshot_preserves_identity_mode_and_targets() {
        let mut device = controller(ALL);
        device.sources = vec!["/dev/input/event9".into(), "/dev/input/event0".into()];
        device.targets = vec!["keyboard0".into(), "gamepad0".into()];
        device.dbus = vec!["dbus1".into(), "dbus0".into()];
        let properties = snapshot_properties(&device);
        device.sources.sort(); device.targets.sort(); device.dbus.sort();
        assert_eq!(controller_info(&device.path, &properties).unwrap(), device);
    }
    #[test]
    fn incomplete_snapshot_never_defaults_identity_or_ownership() {
        for key in ["Name", "ProfileName", "SourceDevicePaths", "TargetDevices", "DbusDevices", "InterceptMode"] {
            let mut properties = snapshot_properties(&controller(ALL));
            properties.remove(key);
            assert!(controller_info("controller0", &properties).is_err(), "missing {key}");
        }
    }
    #[test]
    fn invalid_snapshot_mode_is_a_recoverable_error() {
        let mut properties = snapshot_properties(&controller(ALL));
        properties.insert("InterceptMode".into(), OwnedValue::from(false));
        assert!(controller_info("controller0", &properties).is_err());
    }
    #[test]
    fn recovery_requires_a_presented_menu_and_a_usable_press() {
        assert!(usable_press("ui_down", 1.0, true));
        assert!(usable_press("ui_accept", 1.0, true));
        assert!(usable_press("ui_context", 1.0, true));
        assert!(usable_press("ui_action", 1.0, true));
        assert!(!usable_press("ui_down", 1.0, false));
        assert!(!usable_press("ui_down", 0.0, true));
        assert!(!usable_press("ui_down", 0.5, true));
    }
    #[test]
    fn unrelated_or_invalid_input_does_not_confirm_recovery() {
        assert!(!usable_press("ui_guide", 1.0, true));
        assert!(!usable_press("key_down", 1.0, true));
        assert!(!usable_press("ui_down", f64::NAN, true));
        assert!(!usable_press("ui_down", f64::INFINITY, true));
    }
    fn controller(mode: u32) -> ControllerInfo {
        ControllerInfo { path: "controller0".into(), name: "Controller".into(),
            profile: "Default".into(), sources: vec!["/dev/input/event0".into()],
            targets: vec!["xbox3600".into()], dbus: vec!["dbus0".into()], mode }
    }
    #[test]
    fn hotplug_does_not_take_another_owners_all_mode() {
        assert!(!may_refresh(None, false, &controller(ALL)));
        assert!(may_refresh(None, false, &controller(1)));
        assert!(may_refresh(Some(&controller(1)), true, &controller(ALL)));
    }
    #[test]
    fn hotplug_yields_when_an_existing_interception_is_released() {
        assert!(!may_refresh(Some(&controller(1)), true, &controller(1)));
    }
    #[test]
    fn reused_composite_path_is_not_treated_as_the_old_controller() {
        let old = controller(1);
        let mut replacement = controller(ALL);
        replacement.sources = vec!["/dev/input/event9".into()];
        assert!(!may_refresh(Some(&old), true, &replacement));
    }
    #[test]
    fn missing_leased_controller_detects_loss_for_recovery() {
        let old = controller(ALL);
        let previous = BTreeMap::from([(old.path.clone(), old.clone())]);
        let touched = BTreeSet::from([old.path.clone()]);
        assert!(lost_controller(&previous, &touched, &BTreeMap::new()));
        assert!(!lost_controller(&previous, &touched, &previous));
    }
    #[test]
    fn reused_source_detects_loss_without_restoring_replacement() {
        let old = controller(ALL);
        let previous = BTreeMap::from([(old.path.clone(), old.clone())]);
        let touched = BTreeSet::from([old.path.clone()]);
        let mut replacement = old.clone();
        replacement.sources = vec!["/dev/input/event9".into()];
        assert!(lost_controller(&previous, &touched,
            &BTreeMap::from([(replacement.path.clone(), replacement)])));
    }
    #[test]
    fn lost_normalized_target_detects_loss_for_recovery() {
        let old = controller(ALL);
        let previous = BTreeMap::from([(old.path.clone(), old.clone())]);
        let touched = BTreeSet::from([old.path.clone()]);
        let mut current = old.clone();
        current.dbus.clear();
        assert!(lost_controller(&previous, &touched,
            &BTreeMap::from([(current.path.clone(), current)])));
    }
    #[test]
    fn unleased_removal_and_keyboard_only_start_do_not_report_loss() {
        let old = controller(1);
        let previous = BTreeMap::from([(old.path.clone(), old)]);
        assert!(!lost_controller(&previous, &BTreeSet::new(), &BTreeMap::new()));
        assert!(!lost_controller(&BTreeMap::new(), &BTreeSet::new(), &BTreeMap::new()));
    }
    fn restoration_state() -> (BTreeMap<String, ControllerInfo>, Vec<(String, u32)>, BTreeSet<String>) {
        let device = controller(ALL);
        (BTreeMap::from([(device.path.clone(), device.clone())]),
            vec![(device.path.clone(), 1)], BTreeSet::from([device.path]))
    }
    #[test]
    fn service_restart_discards_all_old_restoration_rights() {
        let (devices, mut saved, mut touched) = restoration_state();
        retain_restoration_rights(&mut saved, &mut touched, &devices, &devices, true);
        assert!(saved.is_empty()); assert!(touched.is_empty());
    }
    #[test]
    fn released_interception_is_reacquired_as_a_new_lease() {
        let (devices, mut saved, mut touched) = restoration_state();
        let mut current = devices.clone();
        current.get_mut("controller0").unwrap().mode = 1;
        retain_restoration_rights(&mut saved, &mut touched, &devices, &current, false);
        assert!(saved.is_empty()); assert!(touched.is_empty());
        assert!(may_refresh(devices.get("controller0"), false, &current["controller0"]));
    }
    #[test]
    fn replacement_already_intercepted_is_left_to_its_owner() {
        let (devices, mut saved, mut touched) = restoration_state();
        let mut current = devices.clone();
        current.get_mut("controller0").unwrap().sources = vec!["new-source".into()];
        retain_restoration_rights(&mut saved, &mut touched, &devices, &current, false);
        assert!(saved.is_empty()); assert!(touched.is_empty());
        assert!(!may_refresh(devices.get("controller0"), false, &current["controller0"]));
    }
    #[test]
    fn disconnect_preserves_the_surviving_controllers_restore_mode() {
        let (mut devices, mut saved, mut touched) = restoration_state();
        let mut second = controller(ALL); second.path = "controller1".into();
        devices.insert(second.path.clone(), second.clone());
        saved.push((second.path.clone(), 0)); touched.insert(second.path.clone());
        let current = BTreeMap::from([(second.path.clone(), second)]);
        retain_restoration_rights(&mut saved, &mut touched, &devices, &current, false);
        assert_eq!(saved, vec![("controller1".into(), 0)]);
        assert_eq!(touched, BTreeSet::from(["controller1".into()]));
    }
    #[test]
    fn direct_unknown_object_becomes_a_controller_loss() {
        let error: error::DynError = Box::new(zbus::fdo::Error::UnknownObject("gone".into()));
        assert!(controller_error(error).is::<ControllerLost>());
    }
    #[test]
    fn replacement_can_be_acquired_after_discarding_old_lease() {
        let old = controller(ALL);
        let mut replacement = controller(1);
        replacement.sources = vec!["/dev/input/event9".into()];
        assert!(may_refresh(Some(&old), false, &replacement));
        replacement.mode = ALL;
        assert!(!may_refresh(Some(&old), false, &replacement));
        assert!(!may_refresh(Some(&old), true, &replacement));
    }
    #[test]
    fn wrapped_unknown_object_is_safe_to_skip_during_restore() {
        let error = zbus::Error::FDO(Box::new(zbus::fdo::Error::UnknownObject("gone".into())));
        assert!(unknown_object(&error));
    }
    #[test]
    fn unrelated_errors_and_message_text_are_not_treated_as_disconnects() {
        let denied = zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied("denied".into())));
        assert!(!unknown_object(&denied));
        let error = message("org.freedesktop.DBus.Error.UnknownObject is just text here");
        assert!(!unknown_object(error.as_ref()));
        assert!(!controller_error(error).is::<ControllerLost>());
    }
    #[test]
    fn foreground_restoration_is_only_owned_for_library_and_game() {
        assert!(owns_foreground("library"));
        assert!(owns_foreground("game"));
        assert!(!owns_foreground("menu"));
        assert!(!owns_foreground("transfer"));
    }
    #[test]
    fn reused_or_vanished_return_window_is_not_focused() {
        let saved = Window { id:20, pid:200, app_id:String::new(), focused:true };
        let tree = json!({"nodes":[{"id":10,"pid":100,"app_id":"org.pegasus-frontend.pegasus-fe"},
            {"id":20,"pid":999,"focused":false}]});
        assert_eq!(restore_target(&tree, Some(&saved), "game", "game"), None);
        assert_eq!(restore_target(&tree, Some(&saved), "game", "library"), Some(10));
    }
    #[test]
    fn target_follows_a_new_session_instead_of_returning_to_the_old_library() {
        let saved = Window { id:10, pid:100, app_id:String::new(), focused:true };
        let tree = json!({"nodes":[{"id":10,"pid":100},
            {"floating_nodes":[{"id":20,"pid":200,"focused":true}]}]});
        assert_eq!(restore_target(&tree, Some(&saved), "library", "game"), Some(20));
    }
}
