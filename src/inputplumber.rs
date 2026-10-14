use crate::error::{message, Result};
use crate::guide::GuideAction;
use futures_util::StreamExt;
use std::collections::{BTreeSet, HashMap};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use zbus::fdo::ObjectManagerProxy;
use zbus::{proxy::CacheProperties, zvariant::OwnedValue, Connection, MessageStream, Proxy};

const SERVICE: &str = "org.shadowblip.InputPlumber";
const ROOT: &str = "/org/shadowblip/InputPlumber";
const COMPOSITE_INTERFACE: &str = "org.shadowblip.Input.CompositeDevice";
const DBUS_DEVICE_INTERFACE: &str = "org.shadowblip.Input.DBusDevice";

pub const INTERCEPT_NONE: u32 = 0;
pub const INTERCEPT_PASS: u32 = 1;
pub const INTERCEPT_ALL: u32 = 2;

fn guide_activation_mode(mode: u32) -> Option<u32> {
    match mode { INTERCEPT_ALL | INTERCEPT_PASS => Some(INTERCEPT_PASS), _ => None }
}

#[cfg(test)]
mod guide_activation_tests {
    use super::*;
    #[test]
    fn activation_restores_pass_and_rejects_other_modes() {
        assert_eq!(guide_activation_mode(INTERCEPT_ALL), Some(INTERCEPT_PASS));
        assert_eq!(guide_activation_mode(INTERCEPT_PASS), Some(INTERCEPT_PASS));
        assert_eq!(guide_activation_mode(INTERCEPT_NONE), None);
        assert_eq!(guide_activation_mode(3), None);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Composite {
    path: String,
    name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory {
    service_owner: String,
    composites: Vec<Composite>,
    dbus_targets: BTreeSet<String>,
}

impl Inventory {
    pub fn composite_count(&self) -> usize {
        self.composites.len()
    }

    pub fn target_count(&self) -> usize {
        self.dbus_targets.len()
    }

    pub fn has_system_input(&self) -> bool {
        !self.composites.is_empty() && !self.dbus_targets.is_empty()
    }

    pub fn log(&self) {
        println!("InputPlumber service owner: {}", self.service_owner);
        for composite in &self.composites {
            println!("Composite: {}", composite.name);
            println!("  runtime path: {}", composite.path);
        }
        for target in &self.dbus_targets {
            println!("  runtime D-Bus target: {target}");
        }

        println!(
            "Discovered {} composite device(s) and {} normalized D-Bus target(s).",
            self.composite_count(),
            self.target_count()
        );
    }
}

#[derive(Clone)]
pub struct InputPlumber {
    connection: Connection,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SystemInputEvent {
    Guide { target: String, action: GuideAction },
    MenuInput { event: String, value: f64 },
}

pub struct SystemInputMonitor {
    receiver: mpsc::Receiver<SystemInputEvent>,
    listeners: JoinSet<()>,
}

impl SystemInputMonitor {
    pub async fn next(&mut self) -> Option<SystemInputEvent> {
        self.receiver.recv().await
    }
}

impl Drop for SystemInputMonitor {
    fn drop(&mut self) {
        self.listeners.abort_all();
    }
}

// A bounded invalidation channel collapses bursts without blocking the bus reader.
pub struct InputInventoryMonitor {
    receiver: mpsc::Receiver<()>,
    listeners: JoinSet<()>,
}

impl InputInventoryMonitor {
    pub async fn next(&mut self) -> Option<()> {
        self.receiver.recv().await
    }
}

impl Drop for InputInventoryMonitor {
    fn drop(&mut self) {
        self.listeners.abort_all();
    }
}

// Keep a request that arrives during discovery until one subsequent scan starts.
// The earliest deadline wins, so a busy hotplug burst cannot postpone it forever.
#[derive(Default)]
pub struct InventoryRefreshQueue {
    deadline: Option<tokio::time::Instant>,
}

impl InventoryRefreshQueue {
    pub fn request(&mut self, now: tokio::time::Instant) {
        self.deadline.get_or_insert(now + Duration::from_millis(100));
    }

    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.deadline
    }

    pub fn started(&mut self) {
        self.deadline = None;
    }
}

fn snapshot_property<T>(properties: &HashMap<String, OwnedValue>, name: &str) -> Result<T>
where
    T: TryFrom<OwnedValue>,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    let value = properties.get(name)
        .ok_or_else(|| message(format!("InputPlumber inventory is missing {name}")))?;
    Ok(T::try_from(value.try_clone()?)?)
}

fn inventory_property_changed<I, N>(interface: &str, names: I) -> bool
where
    I: IntoIterator<Item = N>,
    N: AsRef<str>,
{
    interface == COMPOSITE_INTERFACE
        && names.into_iter().any(|name| matches!(name.as_ref(), "Name" | "DbusDevices" | "SourceDevicePaths"))
}

impl InputPlumber {
    pub async fn connect() -> Result<Self> {
        Ok(Self {
            connection: Connection::system().await?,
        })
    }

    pub async fn discover(&self) -> Result<Inventory> {
        let bus = Proxy::new(
            &self.connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;
        let service_owner: String = bus.call("GetNameOwner", &(SERVICE)).await?;

        let object_manager = ObjectManagerProxy::builder(&self.connection)
            .destination(service_owner.as_str())?
            .path(ROOT)?
            .build()
            .await?;

        let managed_objects = object_manager.get_managed_objects().await?;
        let mut composites = Vec::new();
        let mut dbus_targets = BTreeSet::new();

        for (path, interfaces) in managed_objects {
            let Some(properties) = interfaces.iter()
                .find_map(|(interface, properties)|
                    (interface.as_str() == COMPOSITE_INTERFACE).then_some(properties))
            else {
                continue;
            };

            // GetManagedObjects already contains these properties. Avoid creating
            // per-controller caching proxies and issuing redundant Get/GetAll calls.
            let name: String = snapshot_property(properties, "Name")?;
            let targets: Vec<String> = snapshot_property(properties, "DbusDevices")?;
            for target in targets {
                dbus_targets.insert(target);
            }

            composites.push(Composite {
                path: path.to_string(),
                name,
            });
        }

        let current_owner: String = bus.call("GetNameOwner", &(SERVICE)).await?;
        if current_owner != service_owner {
            return Err(message("InputPlumber restarted during inventory discovery"));
        }

        // ObjectManager does not promise iteration order. A stable inventory
        // prevents harmless ordering changes from rebuilding every listener.
        composites.sort_by(|left, right| left.path.cmp(&right.path));

        Ok(Inventory {
            service_owner,
            composites,
            dbus_targets,
        })
    }

    // Fresh reads are used only when arming/finishing a deliberate Guide close.
    // They prevent a reset release from a removed/replaced source closing the UI.
    pub async fn guide_identity(&self, target: &str) -> Result<Option<String>> {
        let bus = Proxy::new(&self.connection, "org.freedesktop.DBus",
            "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
        let owner: String = bus.call("GetNameOwner", &(SERVICE)).await?;
        let manager = ObjectManagerProxy::builder(&self.connection)
            .destination(owner.as_str())?.path(ROOT)?.build().await?;
        let mut identity = None;
        for (path, interfaces) in manager.get_managed_objects().await? {
            let Some(properties) = interfaces.iter().find_map(|(interface, properties)|
                (interface.as_str() == COMPOSITE_INTERFACE).then_some(properties)) else { continue; };
            let mut targets: Vec<String> = snapshot_property(properties, "DbusDevices")?;
            if !targets.iter().any(|path| path == target) { continue; }
            let mut sources: Vec<String> = snapshot_property(properties, "SourceDevicePaths")?;
            if sources.is_empty() || sources.iter().any(|path|
                path.starts_with("/dev/") && !std::path::Path::new(path).exists()) { continue; }
            let name: String = snapshot_property(properties, "Name")?;
            sources.sort(); targets.sort();
            identity = Some(serde_json::to_string(&(owner.as_str(), path.as_str(), name, sources, targets))?);
            break;
        }
        let current_owner: String = bus.call("GetNameOwner", &(SERVICE)).await?;
        if current_owner != owner { return Err(message("InputPlumber changed during Guide identity read")); }
        Ok(identity)
    }

    // Called only for the deliberate Guide press opening a hidden renderer.
    // Never use this for keyboard opens or to take arbitrary ALL ownership.
    pub async fn release_guide_activation(&self, target: &str) -> Result<()> {
        let identity = self.guide_identity(target).await?
            .ok_or_else(|| message("Guide activation source disappeared"))?;
        let (owner, path, name, sources, targets): (String, String, String, Vec<String>, Vec<String>) =
            serde_json::from_str(&identity)?;
        let proxy = zbus::proxy::Builder::<Proxy<'_>>::new(&self.connection)
            .destination(owner.as_str())?.path(path.as_str())?.interface(COMPOSITE_INTERFACE)?
            .cache_properties(CacheProperties::No).build().await?;
        let mut live_sources: Vec<String> = proxy.get_property("SourceDevicePaths").await?;
        let mut live_targets: Vec<String> = proxy.get_property("DbusDevices").await?;
        let live_name: String = proxy.get_property("Name").await?;
        live_sources.sort(); live_targets.sort();
        if live_sources != sources || live_targets != targets || live_name != name {
            return Err(message("Guide activation source changed"));
        }
        let mode: u32 = proxy.get_property("InterceptMode").await?;
        match guide_activation_mode(mode) {
            Some(next) if next != mode => proxy.set_property("InterceptMode", next).await?,
            Some(_) => {},
            None => return Err(message("Guide activation mode is not PASS/ALL")),
        }
        Ok(())
    }

    pub fn monitor_inventory(&self) -> InputInventoryMonitor {
        let (sender, receiver) = mpsc::channel(1);
        let connection = self.connection.clone();
        let mut listeners = JoinSet::new();
        listeners.spawn(async move {
            let mut last_error = None;
            while !sender.is_closed() {
                match watch_inventory(&connection, &sender).await {
                    Ok(()) => break,
                    Err(error) => {
                        let error = error.to_string();
                        if last_error.as_ref() != Some(&error) {
                            eprintln!("InputPlumber inventory notifications: {error}; retrying.");
                            last_error = Some(error);
                        }
                        let _ = sender.try_send(());
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
        InputInventoryMonitor { receiver, listeners }
    }

    pub async fn set_intercept_mode(&self, inventory: &Inventory, mode: u32) -> Result<()> {
        if mode != INTERCEPT_NONE && !inventory.has_system_input() {
            eprintln!(
                "Skipping InterceptMode {mode}: normalized system input is currently unavailable."
            );
            return Ok(());
        }

        for composite in &inventory.composites {
            let proxy = Proxy::new(
                &self.connection,
                SERVICE,
                composite.path.as_str(),
                COMPOSITE_INTERFACE,
            )
            .await?;

            proxy.set_property("InterceptMode", mode).await?;
            println!("Set {} InterceptMode to {}.", composite.name, mode);
        }

        Ok(())
    }

    pub fn monitor_system_input(
        &self,
        inventory: &Inventory,
        announce: bool,
    ) -> Option<SystemInputMonitor> {
        if !inventory.has_system_input() {
            return None;
        }

        let (sender, receiver) = mpsc::channel(64);
        let mut listeners = JoinSet::new();

        for target in &inventory.dbus_targets {
            let connection = self.connection.clone();
            let target = target.clone();
            let sender = sender.clone();

            listeners.spawn(async move {
                if let Err(error) =
                    monitor_system_input_target(connection, target, sender, announce).await
                {
                    if announce {
                        eprintln!("Input listener failed: {error}");
                    }
                }
            });
        }

        drop(sender);
        Some(SystemInputMonitor {
            receiver,
            listeners,
        })
    }
}

async fn watch_inventory(connection: &Connection, sender: &mpsc::Sender<()>) -> Result<()> {
    // Match rules deliver inventory signals without subscribing to InputEvent
    // or constructing one Properties proxy for every composite device.
    let manager_rule = "type='signal',sender='org.shadowblip.InputPlumber',path='/org/shadowblip/InputPlumber',interface='org.freedesktop.DBus.ObjectManager'";
    let properties_rule = "type='signal',sender='org.shadowblip.InputPlumber',path_namespace='/org/shadowblip/InputPlumber',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.shadowblip.Input.CompositeDevice'";
    let mut objects = MessageStream::for_match_rule(manager_rule, connection, Some(64)).await?;
    let mut properties = MessageStream::for_match_rule(properties_rule, connection, Some(64)).await?;
    let bus = zbus::proxy::Builder::<Proxy<'_>>::new(connection)
        .destination("org.freedesktop.DBus")?
        .path("/org/freedesktop/DBus")?
        .interface("org.freedesktop.DBus")?
        .cache_properties(CacheProperties::No)
        .build().await?;
    let mut owners = bus.receive_signal_with_args("NameOwnerChanged", &[(0, SERVICE)]).await?;
    // A fresh scan after registration closes the initial discovery/subscription gap.
    let _ = sender.try_send(());
    loop {
        tokio::select! {
            _ = sender.closed() => return Ok(()),
            event = objects.next() => {
                let event = event.ok_or_else(|| message("inventory ObjectManager stream closed"))??;
                let header = event.header();
                if matches!(header.member().map(|member| member.as_str()), Some("InterfacesAdded" | "InterfacesRemoved")) {
                    let _ = sender.try_send(());
                }
            }
            event = properties.next() => {
                let event = event.ok_or_else(|| message("inventory property stream closed"))??;
                let (interface, changed, invalidated): (String, HashMap<String, OwnedValue>, Vec<String>) = event.body().deserialize()?;
                // InterceptMode notifications are deliberately excluded: opening
                // the resident menu must not start an inventory reconciliation loop.
                if inventory_property_changed(&interface, changed.keys().chain(invalidated.iter())) {
                    let _ = sender.try_send(());
                }
            }
            event = owners.next() => {
                let event = event.ok_or_else(|| message("inventory owner stream closed"))?;
                let (name, _, _): (String, String, String) = event.body().deserialize()?;
                if name == SERVICE { let _ = sender.try_send(()); }
            }
        }
    }
}

async fn monitor_system_input_target(
    connection: Connection,
    target: String,
    sender: mpsc::Sender<SystemInputEvent>,
    announce: bool,
) -> Result<()> {
    let proxy = Proxy::new(&connection, SERVICE, target.as_str(), DBUS_DEVICE_INTERFACE).await?;
    let mut events = proxy.receive_signal("InputEvent").await?;

    if announce {
        println!("Subscribed to {target}");
    }
    while let Some(signal) = events.next().await {
        let (event, value): (String, f64) = signal.body().deserialize()?;

        if !value.is_finite() || !event.starts_with("ui_") {
            continue;
        }

        if event == "ui_guide" {
            let input = SystemInputEvent::Guide {
                target: target.clone(),
                action: GuideAction::from_input_value(value),
            };
            if sender.send(input).await.is_err() {
                return Ok(());
            }
        } else {
            let input = SystemInputEvent::MenuInput { event, value };
            if sender.send(input).await.is_err() {
                return Ok(());
            }
        }
    }

    Err(message(format!("D-Bus event stream closed: {target}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn incomplete_or_invalid_snapshot_is_rejected() {
        use zbus::zvariant::Value;
        let mut properties = HashMap::from([
            ("Name".into(), OwnedValue::try_from(Value::from("Controller")).unwrap()),
            ("DbusDevices".into(), OwnedValue::try_from(Value::from(vec!["/target/dbus0".to_string()])).unwrap()),
        ]);
        assert_eq!(snapshot_property::<String>(&properties, "Name").unwrap(), "Controller");
        assert_eq!(snapshot_property::<Vec<String>>(&properties, "DbusDevices").unwrap(), vec!["/target/dbus0"]);
        properties.remove("DbusDevices");
        assert!(snapshot_property::<Vec<String>>(&properties, "DbusDevices").is_err());
        properties.insert("Name".into(), OwnedValue::from(false));
        assert!(snapshot_property::<String>(&properties, "Name").is_err());
    }

    #[test]
    fn resident_interception_does_not_invalidate_inventory() {
        assert!(!inventory_property_changed(COMPOSITE_INTERFACE, ["InterceptMode"].into_iter()));
        assert!(!inventory_property_changed(DBUS_DEVICE_INTERFACE, ["Name"].into_iter()));
        for name in ["Name", "DbusDevices", "SourceDevicePaths"] {
            assert!(inventory_property_changed(COMPOSITE_INTERFACE, [name].into_iter()));
        }
    }

    #[test]
    fn hotplug_burst_has_one_deadline_and_inflight_change_survives() {
        let now = tokio::time::Instant::now();
        let mut queue = InventoryRefreshQueue::default();
        queue.request(now);
        queue.request(now + Duration::from_millis(90));
        assert_eq!(queue.deadline(), Some(now + Duration::from_millis(100)));
        queue.started();
        // The first scan is still in flight. Its completion does not clear this request.
        queue.request(now + Duration::from_millis(110));
        assert_eq!(queue.deadline(), Some(now + Duration::from_millis(210)));
        queue.started();
        assert_eq!(queue.deadline(), None);
    }

    #[test]
    fn empty_inventory_is_a_valid_input_unavailable_state() {
        let inventory = Inventory::default();

        assert_eq!(inventory.composite_count(), 0);
        assert_eq!(inventory.target_count(), 0);
        assert!(!inventory.has_system_input());
    }

    #[test]
    fn inventory_identity_tracks_runtime_paths_and_targets() {
        let inventory = Inventory {
            service_owner: ":1.42".to_string(),
            composites: vec![Composite {
                path: "/composite0".to_string(),
                name: "Controller".to_string(),
            }],
            dbus_targets: BTreeSet::from(["/target/dbus0".to_string()]),
        };

        assert_eq!(inventory, inventory.clone());
        let mut restarted = inventory.clone();
        restarted.service_owner = ":1.43".to_string();
        assert_ne!(inventory, restarted);
        assert!(inventory.has_system_input());
    }
}
