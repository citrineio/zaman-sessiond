//! BlueZ settings and an application-scoped pairing agent.
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use zbus::{Connection, Proxy, fdo::ObjectManagerProxy, message::Header};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const AGENT_PATH: &str = "/com/kawnelectro/Zaman/Menu/BluetoothAgent";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub name: String,
    pub powered: bool,
    pub discovering: bool,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Device {
    pub path: String,
    pub adapter: String,
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    pub battery: Option<u8>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BluetoothState {
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
    pub own_scan: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BluetoothAction {
    Read,
    Power { adapter: String, enabled: bool },
    Scan { adapter: String, start: bool },
    Pair(Device),
    Connect(Device),
    Disconnect(Device),
    Forget(Device),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptKind {
    Confirm(u32),
    Authorize,
    Pin,
    Passkey,
    Display { code: String, entered: Option<u16> },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairPrompt {
    pub token: u64,
    pub device: String,
    pub kind: PromptKind,
}
impl PairPrompt {
    pub fn entry(&self) -> bool { matches!(self.kind, PromptKind::Pin | PromptKind::Passkey) }
    pub fn valid_entry(&self, value: &str) -> bool {
        match self.kind {
            PromptKind::Pin => !value.is_empty() && value.len() <= 16 && value.is_ascii(),
            PromptKind::Passkey => value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit()),
            _ => false,
        }
    }
    pub fn detail(&self) -> String {
        match &self.kind {
            PromptKind::Confirm(code) => format!("{}\nCode: {code:06}\nConfirm only if the other device shows the same code.", self.device),
            PromptKind::Authorize => format!("{}\nAllow this pairing request?", self.device),
            PromptKind::Pin => format!("{}\nEnter its pairing PIN, then select Confirm.", self.device),
            PromptKind::Passkey => format!("{}\nEnter its six-digit code, then select Confirm.", self.device),
            PromptKind::Display { code, entered } => format!("{}\nType {code} on the other device, then press Enter.{}", self.device,
                entered.map(|count| format!("\n{count} digits entered.")).unwrap_or_default()),
        }
    }
}
pub enum BluetoothEvent {
    State { generation: u64, operation: u64, result: std::result::Result<BluetoothState, String> },
    Prompt { generation: u64, operation: u64, prompt: PairPrompt, reply: Option<oneshot::Sender<Option<String>>> },
    Dismiss { generation: u64, operation: u64 },
}

struct Scan {
    token: u64,
    epoch: u64,
    connection: Connection,
    adapter: String,
}
#[derive(Clone, Default)]
pub struct BluetoothClient {
    scan: Arc<tokio::sync::Mutex<Option<Scan>>>,
    cancel_pair: Arc<Mutex<Option<(u64, u64, oneshot::Sender<()>)>>>,
    sequence: Arc<AtomicU64>,
    epoch: Arc<AtomicU64>,
}
impl BluetoothClient {
    pub fn cancel_pairing(&self) {
        if let Some((_, _, cancel)) = self.cancel_pair.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = cancel.send(());
        }
    }
    pub fn close(&self) {
        let closed_epoch = self.epoch.fetch_add(1, Ordering::Relaxed);
        self.cancel_pairing();
        let client = self.clone();
        tokio::spawn(async move {
            let scan = {
                let mut held = client.scan.lock().await;
                if held.as_ref().is_some_and(|scan| scan.epoch <= closed_epoch) { held.take() } else { None }
            };
            if let Some(scan) = scan {
                if let Ok(proxy) = Proxy::new(&scan.connection, BLUEZ, scan.adapter.as_str(), ADAPTER).await {
                    let _ = tokio::time::timeout(Duration::from_secs(5), proxy.call::<_, _, ()>("StopDiscovery", &())).await;
                }
            }
        });
    }
    async fn stop_scan(&self) -> Result<()> {
        if let Some(scan) = self.scan.lock().await.take() {
            let proxy = Proxy::new(&scan.connection, BLUEZ, scan.adapter.as_str(), ADAPTER).await?;
            let _: () = tokio::time::timeout(Duration::from_secs(5), proxy.call("StopDiscovery", &())).await??;
        }
        Ok(())
    }
    pub fn start(&self, action: BluetoothAction, generation: u64, operation: u64, events: mpsc::UnboundedSender<BluetoothEvent>) {
        let client = self.clone();
        let epoch = self.epoch.load(Ordering::Relaxed);
        // Install cancellation before spawning, so immediate Back can cancel
        // even while the system bus/agent is still being initialized.
        let cancel = if matches!(action, BluetoothAction::Pair(_)) {
            let (send, receive) = oneshot::channel();
            *self.cancel_pair.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, operation, send));
            Some(receive)
        } else { None };
        tokio::spawn(async move {
            let pairing = matches!(action, BluetoothAction::Pair(_));
            let result = client.apply(action, generation, operation, epoch, events.clone(), cancel).await.map_err(|error| error.to_string());
            if pairing {
                let mut held = client.cancel_pair.lock().unwrap_or_else(|e| e.into_inner());
                if held.as_ref().is_some_and(|(active_generation, active_operation, _)| *active_generation == generation && *active_operation == operation) { held.take(); }
            }
            let _ = events.send(BluetoothEvent::State { generation, operation, result });
        });
    }
    async fn read(&self) -> Result<BluetoothState> {
        let connection = Connection::system().await?;
        let mut state = tokio::time::timeout(Duration::from_secs(8), inventory(&connection)).await??;
        state.own_scan = self.scan.lock().await.is_some();
        Ok(state)
    }
    async fn apply(&self, action: BluetoothAction, generation: u64, operation: u64, epoch: u64,
        events: mpsc::UnboundedSender<BluetoothEvent>, cancel: Option<oneshot::Receiver<()>>) -> Result<BluetoothState> {
        if action == BluetoothAction::Read { return self.read().await; }
        if let BluetoothAction::Scan { start: false, .. } = &action {
            self.stop_scan().await?;
            return self.read().await;
        }
        let connection = Connection::system().await?;
        let device_method = match &action { BluetoothAction::Disconnect(_) => "Disconnect", BluetoothAction::Forget(_) => "RemoveDevice", _ => "Connect" };
        match action {
            BluetoothAction::Power { adapter, enabled } => {
                if !enabled { self.stop_scan().await?; }
                let proxy = Proxy::new(&connection, BLUEZ, adapter.as_str(), ADAPTER).await?;
                tokio::time::timeout(Duration::from_secs(8), proxy.set_property("Powered", enabled)).await??;
            }
            BluetoothAction::Scan { adapter, start: true } => {
                self.stop_scan().await?;
                let proxy = Proxy::new(&connection, BLUEZ, adapter.as_str(), ADAPTER).await?;
                let _: () = tokio::time::timeout(Duration::from_secs(8), proxy.call("StartDiscovery", &())).await??;
                let token = self.sequence.fetch_add(1, Ordering::Relaxed);
                let mut held = self.scan.lock().await;
                if self.epoch.load(Ordering::Relaxed) != epoch {
                    let _ = tokio::time::timeout(Duration::from_secs(5), proxy.call::<_, _, ()>("StopDiscovery", &())).await;
                    return Err(io::Error::other("Bluetooth settings closed.").into());
                }
                *held = Some(Scan { token, epoch, connection: connection.clone(), adapter });
                drop(held);
                let client = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let scan = {
                        let mut held = client.scan.lock().await;
                        if held.as_ref().is_some_and(|scan| scan.token == token) { held.take() } else { None }
                    };
                    if let Some(scan) = scan {
                        if let Ok(proxy) = Proxy::new(&scan.connection, BLUEZ, scan.adapter.as_str(), ADAPTER).await {
                            let _ = tokio::time::timeout(Duration::from_secs(5), proxy.call::<_, _, ()>("StopDiscovery", &())).await;
                        }
                    }
                });
            }
            BluetoothAction::Pair(device) => {
                self.stop_scan().await?;
                validate_device(&connection, &device).await?;
                pair(&connection, &device, generation, operation, events, cancel.ok_or_else(|| io::Error::other("Missing pairing cancellation."))?).await?;
                let proxy = Proxy::new(&connection, BLUEZ, device.path.as_str(), DEVICE).await?;
                tokio::time::timeout(Duration::from_secs(8), proxy.set_property("Trusted", true)).await??;
            }
            BluetoothAction::Connect(device) | BluetoothAction::Disconnect(device) | BluetoothAction::Forget(device) => {
                validate_device(&connection, &device).await?;
                let method = device_method;
                if method == "RemoveDevice" {
                    let proxy = Proxy::new(&connection, BLUEZ, device.adapter.as_str(), ADAPTER).await?;
                    let path = OwnedObjectPath::try_from(device.path)?;
                    let _: () = tokio::time::timeout(Duration::from_secs(20), proxy.call(method, &(path,))).await??;
                } else {
                    let proxy = Proxy::new(&connection, BLUEZ, device.path.as_str(), DEVICE).await?;
                    let _: () = tokio::time::timeout(Duration::from_secs(20), proxy.call(method, &())).await??;
                }
            }
            _ => {}
        }
        self.read().await
    }
}

fn text(properties: &HashMap<String, OwnedValue>, key: &str) -> String {
    properties.get(key).and_then(|value| <&str>::try_from(value).ok()).unwrap_or("").to_owned()
}
fn flag(properties: &HashMap<String, OwnedValue>, key: &str) -> bool {
    properties.get(key).and_then(|value| bool::try_from(value).ok()).unwrap_or(false)
}
fn clean(name: String) -> String { name.chars().filter(|c| !c.is_control()).take(64).collect() }
fn device_name(alias: String, fallback: &str) -> String {
    let name = clean(alias);
    if name.is_empty() { fallback.to_owned() } else { name }
}
async fn inventory(connection: &Connection) -> Result<BluetoothState> {
    let manager = ObjectManagerProxy::builder(connection).destination(BLUEZ)?.path("/")?.build().await?;
    let objects = manager.get_managed_objects().await?;
    let mut adapters = Vec::new();
    let mut devices = Vec::new();
    for (path, interfaces) in objects {
        if let Some(props) = interfaces.iter().find(|(interface, _)| interface.as_str() == ADAPTER).map(|(_, props)| props) {
            let alias = text(props, "Alias");
            adapters.push(Adapter { path: path.to_string(), name: device_name(if alias.is_empty() { text(props, "Name") } else { alias }, "Bluetooth adapter"),
                powered: flag(props, "Powered"), discovering: flag(props, "Discovering") });
        }
        if let Some(props) = interfaces.iter().find(|(interface, _)| interface.as_str() == DEVICE).map(|(_, props)| props) {
            let address = text(props, "Address");
            let alias = text(props, "Alias");
            let adapter = props.get("Adapter").and_then(|value| value.try_clone().ok()).and_then(|value| OwnedObjectPath::try_from(value).ok()).map(|path| path.to_string()).unwrap_or_default();
            let battery = interfaces.iter().find(|(interface, _)| interface.as_str() == "org.bluez.Battery1")
                .and_then(|(_, props)| props.get("Percentage")).and_then(|value| u8::try_from(value).ok());
            devices.push(Device { path: path.to_string(), adapter, address: address.clone(), name: device_name(alias, &address),
                paired: flag(props, "Paired"), connected: flag(props, "Connected"), trusted: flag(props, "Trusted"), battery });
        }
    }
    adapters.sort_by(|a, b| b.powered.cmp(&a.powered).then(a.path.cmp(&b.path)));
    let adapter = adapters.into_iter().next();
    devices.retain(|device| adapter.as_ref().is_some_and(|adapter| adapter.path == device.adapter));
    devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.paired.cmp(&a.paired)).then(a.name.cmp(&b.name)).then(a.address.cmp(&b.address)));
    Ok(BluetoothState { adapter, devices, own_scan: false })
}
async fn validate_device(connection: &Connection, selected: &Device) -> Result<()> {
    let proxy = Proxy::new(connection, BLUEZ, selected.path.as_str(), DEVICE).await?;
    let address: String = proxy.get_property("Address").await?;
    let adapter: OwnedObjectPath = proxy.get_property("Adapter").await?;
    if address != selected.address || adapter.as_str() != selected.adapter {
        return Err(io::Error::other("The selected Bluetooth device is no longer available.").into());
    }
    Ok(())
}

fn selected_request(owner: &str, sender: Option<&str>, selected: &str, device: Option<&str>) -> bool {
    sender == Some(owner) && device.is_none_or(|device| device == selected)
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError { Rejected(String), Canceled(String) }
type AgentResult<T> = std::result::Result<T, AgentError>;
struct PairAgent {
    owner: String,
    device: Device,
    generation: u64,
    operation: u64,
    events: mpsc::UnboundedSender<BluetoothEvent>,
    sequence: AtomicU64,
}
impl PairAgent {
    fn check(&self, header: &Header<'_>, device: Option<&OwnedObjectPath>) -> AgentResult<()> {
        if !selected_request(&self.owner, header.sender().map(|sender| sender.as_str()),
            &self.device.path, device.map(|device| device.as_str())) {
            return Err(AgentError::Rejected("Only the selected device may pair through this menu.".into()));
        }
        Ok(())
    }
    fn event(&self, kind: PromptKind, reply: Option<oneshot::Sender<Option<String>>>) -> AgentResult<()> {
        let prompt = PairPrompt { token: self.sequence.fetch_add(1, Ordering::Relaxed), device: self.device.name.chars().take(32).collect(), kind };
        self.events.send(BluetoothEvent::Prompt { generation: self.generation, operation: self.operation, prompt, reply })
            .map_err(|_| AgentError::Canceled("The menu closed.".into()))
    }
    async fn ask(&self, kind: PromptKind) -> AgentResult<String> {
        let (send, receive) = oneshot::channel();
        self.event(kind, Some(send))?;
        tokio::time::timeout(Duration::from_secs(90), receive).await
            .map_err(|_| AgentError::Canceled("Pairing confirmation timed out.".into()))?
            .map_err(|_| AgentError::Canceled("The menu closed.".into()))?
            .ok_or_else(|| AgentError::Rejected("Pairing was canceled.".into()))
    }
    fn dismiss(&self) { let _ = self.events.send(BluetoothEvent::Dismiss { generation: self.generation, operation: self.operation }); }
}
#[zbus::interface(name = "org.bluez.Agent1")]
// zbus 5.5.0 recognizes fallible interface methods syntactically by the
// final type name `Result`; `AgentResult` is only safe in ordinary helpers.
impl PairAgent {
    fn release(&self, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> { self.check(&header, None)?; self.dismiss(); Ok(()) }
    async fn request_pin_code(&self, device: OwnedObjectPath, #[zbus(header)] header: Header<'_>) -> std::result::Result<String, AgentError> {
        self.check(&header, Some(&device))?;
        let value = self.ask(PromptKind::Pin).await?;
        if value.is_empty() || value.len() > 16 || !value.is_ascii() { return Err(AgentError::Rejected("Invalid PIN.".into())); }
        Ok(value)
    }
    async fn request_passkey(&self, device: OwnedObjectPath, #[zbus(header)] header: Header<'_>) -> std::result::Result<u32, AgentError> {
        self.check(&header, Some(&device))?;
        let value = self.ask(PromptKind::Passkey).await?;
        if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) { return Err(AgentError::Rejected("Invalid passkey.".into())); }
        value.parse().map_err(|_| AgentError::Rejected("Invalid passkey.".into()))
    }
    fn display_pin_code(&self, device: OwnedObjectPath, pincode: String, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> {
        self.check(&header, Some(&device))?;
        self.event(PromptKind::Display { code: clean(pincode).chars().take(16).collect(), entered: None }, None)
    }
    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, entered: u16, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> {
        self.check(&header, Some(&device))?;
        self.event(PromptKind::Display { code: format!("{passkey:06}"), entered: Some(entered) }, None)
    }
    async fn request_confirmation(&self, device: OwnedObjectPath, passkey: u32, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> {
        self.check(&header, Some(&device))?;
        self.ask(PromptKind::Confirm(passkey)).await.map(|_| ())
    }
    async fn request_authorization(&self, device: OwnedObjectPath, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> {
        self.check(&header, Some(&device))?;
        self.ask(PromptKind::Authorize).await.map(|_| ())
    }
    fn authorize_service(&self, device: OwnedObjectPath, _uuid: String, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> {
        // Pairing was explicitly requested for this device; never become
        // the system default agent or authorize unrelated incoming devices.
        self.check(&header, Some(&device))
    }
    fn cancel(&self, #[zbus(header)] header: Header<'_>) -> std::result::Result<(), AgentError> { self.check(&header, None)?; self.dismiss(); Ok(()) }
}
async fn pair(connection: &Connection, device: &Device, generation: u64, operation: u64,
    events: mpsc::UnboundedSender<BluetoothEvent>, mut cancel: oneshot::Receiver<()>) -> Result<()> {
    if cancel.try_recv().is_ok() { return Err(io::Error::other("Pairing canceled.").into()); }
    let bus = Proxy::new(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
    let owner: String = bus.call("GetNameOwner", &(BLUEZ,)).await?;
    connection.object_server().at(AGENT_PATH, PairAgent { owner, device: device.clone(), generation, operation, events: events.clone(), sequence: AtomicU64::new(1) }).await?;
    let manager = Proxy::new(connection, BLUEZ, "/org/bluez", "org.bluez.AgentManager1").await?;
    let agent = OwnedObjectPath::try_from(AGENT_PATH)?;
    let _: () = tokio::time::timeout(Duration::from_secs(8), manager.call("RegisterAgent", &(agent.clone(), "KeyboardDisplay"))).await??;
    let proxy = Proxy::new(connection, BLUEZ, device.path.as_str(), DEVICE).await?;
    let result: Result<()> = tokio::select! {
        _ = &mut cancel => Err(io::Error::other("Pairing canceled.").into()),
        result = tokio::time::timeout(Duration::from_secs(120), proxy.call::<_, _, ()>("Pair", &())) => {
            match result {
                Ok(result) => result.map_err(Into::into),
                Err(_) => Err(io::Error::other("Pairing timed out.").into()),
            }
        }
    };
    // Release a pending UI reply before removing the agent; its method may
    // hold the interface read lock while awaiting confirmation.
    let _ = events.send(BluetoothEvent::Dismiss { generation, operation });
    if result.is_err() { let _ = tokio::time::timeout(Duration::from_secs(5), proxy.call::<_, _, ()>("CancelPairing", &())).await; }
    let _ = tokio::time::timeout(Duration::from_secs(5), manager.call::<_, _, ()>("UnregisterAgent", &(agent,))).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), connection.object_server().remove::<PairAgent, _>(AGENT_PATH)).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_rejects_other_senders_and_devices() {
        assert!(selected_request(":1.10", Some(":1.10"), "/org/bluez/hci0/dev_A", Some("/org/bluez/hci0/dev_A")));
        assert!(!selected_request(":1.10", Some(":1.99"), "/org/bluez/hci0/dev_A", Some("/org/bluez/hci0/dev_A")));
        assert!(!selected_request(":1.10", Some(":1.10"), "/org/bluez/hci0/dev_A", Some("/org/bluez/hci0/dev_B")));
        assert!(!selected_request(":1.10", None, "/org/bluez/hci0/dev_A", None));
        assert!(selected_request(":1.10", Some(":1.10"), "/org/bluez/hci0/dev_A", None));
    }
    #[test]
    fn pairing_entries_have_protocol_specific_limits() {
        let mut prompt = PairPrompt { token: 1, device: "Keyboard".into(), kind: PromptKind::Pin };
        assert!(prompt.valid_entry("0000"));
        assert!(!prompt.valid_entry(""));
        assert!(!prompt.valid_entry("12345678901234567"));
        prompt.kind = PromptKind::Passkey;
        assert!(prompt.valid_entry("000123"));
        assert!(!prompt.valid_entry("123"));
        assert!(!prompt.valid_entry("12x456"));
        assert!(prompt.detail().contains("six-digit"));
        prompt.kind = PromptKind::Confirm(123);
        assert!(prompt.detail().contains("000123"));
    }
}
