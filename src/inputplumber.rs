use crate::error::{message, Result};
use crate::guide::GuideAction;
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
            let targets: Vec<String> = proxy.get_property("DbusDevices").await?;

            for target in targets {
                dbus_targets.insert(target);
            }

            composites.push(Composite {
                path: path.to_string(),
                name,
            });
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

        if !event.starts_with("ui_") {
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
    use super::{Composite, Inventory};
    use std::collections::BTreeSet;

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
