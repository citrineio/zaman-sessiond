use crate::error::{message, Result};
use crate::guide::{GuideAction, GuideButton};
use futures_util::StreamExt;
use std::collections::BTreeSet;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use zbus::fdo::ObjectManagerProxy;
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.shadowblip.InputPlumber";
const ROOT: &str = "/org/shadowblip/InputPlumber";
const COMPOSITE_INTERFACE: &str = "org.shadowblip.Input.CompositeDevice";
const DBUS_DEVICE_INTERFACE: &str = "org.shadowblip.Input.DBusDevice";

pub const INTERCEPT_NONE: u32 = 0;
pub const INTERCEPT_PASS: u32 = 1;
pub const INTERCEPT_ALL: u32 = 2;

#[derive(Clone, Debug)]
struct Composite {
    path: String,
    name: String,
}

#[derive(Clone, Debug)]
pub struct Inventory {
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
}

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
    pub async fn next(&mut self) -> SystemInputEvent {
        if let Some(event) = self.receiver.recv().await {
            return event;
        }

        eprintln!(
            "All InputPlumber event listeners stopped; continuing without system input supervision."
        );
        std::future::pending::<SystemInputEvent>().await
    }
}

impl Drop for SystemInputMonitor {
    fn drop(&mut self) {
        self.listeners.abort_all();
    }
}

impl InputPlumber {
    pub async fn connect() -> Result<Self> {
        Ok(Self {
            connection: Connection::system().await?,
        })
    }

    pub async fn discover(&self) -> Result<Inventory> {
        let object_manager = ObjectManagerProxy::builder(&self.connection)
            .destination(SERVICE)?
            .path(ROOT)?
            .build()
            .await?;

        let managed_objects = object_manager.get_managed_objects().await?;
        let mut composites = Vec::new();
        let mut dbus_targets = BTreeSet::new();

        for (path, interfaces) in managed_objects {
            let is_composite = interfaces
                .keys()
                .any(|interface| interface.as_str() == COMPOSITE_INTERFACE);

            if !is_composite {
                continue;
            }

            let proxy = Proxy::new(
                &self.connection,
                SERVICE,
                path.as_str(),
                COMPOSITE_INTERFACE,
            )
            .await?;

            let name: String = proxy.get_property("Name").await?;
            let persistent_id: String = proxy.get_property("PersistentId").await?;
            let targets: Vec<String> = proxy.get_property("DbusDevices").await?;

            println!("Composite: {name}");
            println!("  runtime path: {path}");
            println!("  persistent ID: {persistent_id}");

            for target in targets {
                println!("  runtime D-Bus target: {target}");
                dbus_targets.insert(target);
            }

            composites.push(Composite {
                path: path.to_string(),
                name,
            });
        }

        if composites.is_empty() {
            eprintln!(
                "InputPlumber currently reports no composite devices; continuing without system input."
            );
        } else if dbus_targets.is_empty() {
            eprintln!(
                "InputPlumber currently reports no normalized D-Bus input targets; continuing without system input."
            );
        }

        Ok(Inventory {
            composites,
            dbus_targets,
        })
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

    pub fn monitor_system_input(&self, inventory: &Inventory) -> SystemInputMonitor {
        let (sender, receiver) = mpsc::channel(64);
        let mut listeners = JoinSet::new();

        if !inventory.has_system_input() {
            eprintln!(
                "Guide supervision deferred: normalized system input is currently unavailable."
            );
        } else {
            for target in &inventory.dbus_targets {
                let connection = self.connection.clone();
                let target = target.clone();
                let sender = sender.clone();

                listeners.spawn(async move {
                    if let Err(error) =
                        monitor_system_input_target(connection, target, sender).await
                    {
                        eprintln!("Input listener failed: {error}");
                    }
                });
            }
        }

        drop(sender);
        SystemInputMonitor {
            receiver,
            listeners,
        }
    }
}

async fn monitor_system_input_target(
    connection: Connection,
    target: String,
    sender: mpsc::Sender<SystemInputEvent>,
) -> Result<()> {
    let proxy = Proxy::new(&connection, SERVICE, target.as_str(), DBUS_DEVICE_INTERFACE).await?;
    let mut events = proxy.receive_signal("InputEvent").await?;

    println!("Subscribed to {target}");
    let mut guide = GuideButton::default();

    while let Some(signal) = events.next().await {
        let (event, value): (String, f64) = signal.body().deserialize()?;

        if !event.starts_with("ui_") {
            continue;
        }

        if event == "ui_guide" {
            let actions = if value > 0.5 {
                guide.press()
            } else {
                guide.release()
            };

            if !send_guide_actions(&sender, &target, actions).await {
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

async fn send_guide_actions(
    sender: &mpsc::Sender<SystemInputEvent>,
    target: &str,
    actions: Vec<GuideAction>,
) -> bool {
    for action in actions {
        let event = SystemInputEvent::Guide {
            target: target.to_string(),
            action,
        };
        if sender.send(event).await.is_err() {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::Inventory;
    use std::collections::BTreeSet;

    #[test]
    fn empty_inventory_is_a_valid_input_unavailable_state() {
        let inventory = Inventory {
            composites: Vec::new(),
            dbus_targets: BTreeSet::new(),
        };

        assert_eq!(inventory.composite_count(), 0);
        assert_eq!(inventory.target_count(), 0);
        assert!(!inventory.has_system_input());
    }
}
