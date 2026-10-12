//! One-shot, application-scoped Bluetooth pairing agent for the development menu.
//! Discovery and device management remain native Quickshell operations.
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use zbus::{Connection, Proxy, message::Header};
use zbus::zvariant::OwnedObjectPath;
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BLUEZ: &str = "org.bluez";
const DEVICE: &str = "org.bluez.Device1";
const AGENT_PATH: &str = "/com/kawnelectro/Zaman/Preview/PairAgent";
#[derive(Clone)]
enum PromptKind {
    Confirm(u32), Authorize, Pin, Passkey,
    Display { code: String, entered: Option<u16> },
}
enum AgentEvent {
    Prompt { token: u64, kind: PromptKind, reply: Option<oneshot::Sender<Option<String>>> },
    Dismiss,
}
fn clean(text: String) -> String { text.chars().filter(|c| !c.is_control()).take(180).collect() }
fn emit(value: Value) -> Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}
fn valid_entry(kind: &PromptKind, value: &str) -> bool {
    match kind {
        PromptKind::Pin => !value.is_empty() && value.len() <= 16 && value.bytes().all(|c| (32..=126).contains(&c)),
        PromptKind::Passkey => value.len() == 6 && value.bytes().all(|c| c.is_ascii_digit()),
        PromptKind::Confirm(_) | PromptKind::Authorize => value == "accept",
        PromptKind::Display { .. } => false,
    }
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
    device: String,
    events: mpsc::UnboundedSender<AgentEvent>,
    sequence: AtomicU64,
}
impl PairAgent {
    fn check(&self, header: &Header<'_>, device: Option<&OwnedObjectPath>) -> AgentResult<()> {
        if !selected_request(&self.owner, header.sender().map(|sender| sender.as_str()),
            &self.device, device.map(|device| device.as_str())) {
            return Err(AgentError::Rejected("Only the selected device may pair through this menu.".into()));
        }
        Ok(())
    }
    fn event(&self, kind: PromptKind, reply: Option<oneshot::Sender<Option<String>>>) -> AgentResult<()> {
        let token = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.events.send(AgentEvent::Prompt { token, kind, reply })
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
    fn dismiss(&self) { let _ = self.events.send(AgentEvent::Dismiss); }
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

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 { return Err(io::Error::other("Pairing helper requires device, adapter and address.").into()); }
    let path = OwnedObjectPath::try_from(args[0].clone())?;
    let adapter = OwnedObjectPath::try_from(args[1].clone())?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let (commands_tx, mut commands) = mpsc::channel(8);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.len() > 4096 || commands_tx.blocking_send(line).is_err() { break; }
        }
    });
    let (events_tx, mut events) = mpsc::unbounded_channel();
    let (connection, owner) = tokio::time::timeout(Duration::from_secs(6), async {
        let connection = Connection::system().await?;
        let bus = Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
        let owner: String = bus.call("GetNameOwner", &(BLUEZ,)).await?;
        drop(bus);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((connection, owner))
    }).await??;
    // All registration, identity reads and mutations stay pinned to this owner.
    let proxy = Proxy::new(&connection, owner.as_str(), path.as_str(), DEVICE).await?;
    tokio::time::timeout(Duration::from_secs(6), async {
        let address: String = proxy.get_property("Address").await?;
        let actual_adapter: OwnedObjectPath = proxy.get_property("Adapter").await?;
        if address != args[2] || actual_adapter != adapter {
            return Err(io::Error::other("The selected Bluetooth device changed.").into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }).await??;
    connection.object_server().at(AGENT_PATH, PairAgent {
        owner: owner.clone(), device: path.to_string(), events: events_tx,
        sequence: AtomicU64::new(1),
    }).await?;
    let manager = Proxy::new(&connection, owner.as_str(), "/org/bluez", "org.bluez.AgentManager1").await?;
    let agent = OwnedObjectPath::try_from(AGENT_PATH)?;
    let _: () = tokio::time::timeout(Duration::from_secs(6), manager.call("RegisterAgent", &(agent.clone(), "KeyboardDisplay"))).await??;
    let mut pending: Option<(u64, PromptKind, oneshot::Sender<Option<String>>)> = None;
    let result: Result<()> = async {
        emit(json!({"type":"ready"}))?;
        let pairing = tokio::time::timeout(Duration::from_secs(120), proxy.call::<_, _, ()>("Pair", &()));
        tokio::pin!(pairing);
        let mut last_heartbeat = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _ = term.recv() => return Err(io::Error::other("Pairing canceled.").into()),
                _ = interrupt.recv() => return Err(io::Error::other("Pairing canceled.").into()),
                _ = tick.tick() => {
                    if last_heartbeat.elapsed() > Duration::from_secs(5) {
                        return Err(io::Error::other("Pairing canceled because the menu stopped responding.").into());
                    }
                }
                command = commands.recv() => {
                    let Some(line) = command else { return Err(io::Error::other("The menu closed.").into()); };
                    let Ok(command) = serde_json::from_str::<Value>(&line) else { continue };
                    match command["action"].as_str() {
                        Some("heartbeat") => last_heartbeat = Instant::now(),
                        Some("cancel") => return Err(io::Error::other("Pairing canceled.").into()),
                        Some("reply") => {
                            let token = command["token"].as_u64();
                            if pending.as_ref().is_some_and(|(id, _, _)| Some(*id) == token) {
                                let value = command["value"].as_str();
                                if let Some((_, kind, _)) = pending.as_ref() {
                                    if value.is_some_and(|value| !valid_entry(kind, value)) { continue; }
                                }
                                if let Some((_, _, reply)) = pending.take() { let _ = reply.send(value.map(str::to_owned)); }
                            }
                        }
                        _ => {},
                    }
                }
                result = &mut pairing => { result??; break; }
                event = events.recv() => match event {
                    Some(AgentEvent::Prompt { token, kind, reply }) => {
                        pending.take();
                        let message = match &kind {
                            PromptKind::Confirm(code) => json!({"type":"prompt","token":token,"kind":"confirm","code":format!("{code:06}")}),
                            PromptKind::Authorize => json!({"type":"prompt","token":token,"kind":"authorize"}),
                            PromptKind::Pin => json!({"type":"prompt","token":token,"kind":"pin"}),
                            PromptKind::Passkey => json!({"type":"prompt","token":token,"kind":"passkey"}),
                            PromptKind::Display { code, entered } => json!({"type":"prompt","token":token,"kind":"display","code":code,"entered":entered}),
                        };
                        if let Some(reply) = reply { pending = Some((token, kind, reply)); }
                        emit(message)?;
                    }
                    Some(AgentEvent::Dismiss) => { pending.take(); emit(json!({"type":"dismiss"}))?; }
                    None => return Err(io::Error::other("Pairing agent stopped.").into()),
                },
            }
        }
        // A successful Pair reply alone is not our UI's connection indication.
        let paired = tokio::time::timeout(Duration::from_secs(3), async {
            let properties = Proxy::new(&connection, owner.as_str(), path.as_str(), "org.freedesktop.DBus.Properties").await?;
            let value: zbus::zvariant::OwnedValue = properties.call("Get", &(DEVICE, "Paired")).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(bool::try_from(value)?)
        }).await??;
        if !paired { return Err(io::Error::other("Pairing was not confirmed by the device service.").into()); }
        Ok(())
    }.await;
    // Release pending method handlers before removing their interface.
    pending.take();
    if result.is_err() {
        let _ = tokio::time::timeout(Duration::from_secs(3), proxy.call::<_, _, ()>("CancelPairing", &())).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(3), manager.call::<_, _, ()>("UnregisterAgent", &(agent,))).await;
    let _ = tokio::time::timeout(Duration::from_secs(3), connection.object_server().remove::<PairAgent, _>(AGENT_PATH)).await;
    result?;
    emit(json!({"type":"paired"}))?;
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        let _ = emit(json!({"type":"error","message":clean(error.to_string())}));
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_selected_device_and_owner_may_prompt() {
        assert!(selected_request(":1.10", Some(":1.10"), "/dev_A", Some("/dev_A")));
        assert!(!selected_request(":1.10", Some(":1.11"), "/dev_A", Some("/dev_A")));
        assert!(!selected_request(":1.10", Some(":1.10"), "/dev_A", Some("/dev_B")));
        assert!(!selected_request(":1.10", None, "/dev_A", None));
    }
    #[test]
    fn replies_have_protocol_specific_limits() {
        assert!(valid_entry(&PromptKind::Pin, "A123"));
        assert!(!valid_entry(&PromptKind::Pin, ""));
        assert!(!valid_entry(&PromptKind::Pin, "12345678901234567"));
        assert!(!valid_entry(&PromptKind::Pin, "hello\n"));
        assert!(valid_entry(&PromptKind::Passkey, "000123"));
        assert!(!valid_entry(&PromptKind::Passkey, "123"));
        assert!(!valid_entry(&PromptKind::Passkey, "12x456"));
        assert!(valid_entry(&PromptKind::Confirm(123), "accept"));
        assert!(!valid_entry(&PromptKind::Authorize, "yes"));
    }
}
