use crate::error::{message, Result};
use futures_util::StreamExt;
use std::collections::BTreeSet;
use tokio::task::JoinSet;
use zbus::fdo::ObjectManagerProxy;
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.shadowblip.InputPlumber";
const ROOT: &str = "/org/shadowblip/InputPlumber";
const COMPOSITE_INTERFACE: &str = "org.shadowblip.Input.CompositeDevice";
const DBUS_DEVICE_INTERFACE: &str = "org.shadowblip.Input.DBusDevice";

pub const INTERCEPT_NONE: u32 = 0;
pub const INTERCEPT_PASS: u32 = 1;

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
}

pub struct InputPlumber {
    connection: Connection,
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
            return Err(message("InputPlumber reported no composite devices"));
        }

        if dbus_targets.is_empty() {
            return Err(message(
                "InputPlumber reported no normalized D-Bus input targets",
            ));
        }

        Ok(Inventory {
            composites,
            dbus_targets,
        })
    }

    pub async fn set_intercept_mode(&self, inventory: &Inventory, mode: u32) -> Result<()> {
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

    pub async fn wait_for_guide(&self, inventory: &Inventory) -> Result<String> {
        let mut listeners = JoinSet::new();

        for target in &inventory.dbus_targets {
            let connection = self.connection.clone();
            let target = target.clone();

            listeners.spawn(async move { wait_for_guide(connection, target).await });
        }

        while let Some(completed) = listeners.join_next().await {
            match completed {
                Ok(Ok(target)) => {
                    listeners.abort_all();
                    return Ok(target);
                }
                Ok(Err(error)) => eprintln!("Input listener failed: {error}"),
                Err(error) => eprintln!("Input listener task failed: {error}"),
            }
        }

        Err(message("all InputPlumber event listeners stopped"))
    }
}

async fn wait_for_guide(connection: Connection, target: String) -> Result<String> {
    let proxy = Proxy::new(&connection, SERVICE, target.as_str(), DBUS_DEVICE_INTERFACE).await?;
    let mut events = proxy.receive_signal("InputEvent").await?;

    println!("Subscribed to {target}");

    while let Some(message) = events.next().await {
        let (event, value): (String, f64) = message.body().deserialize()?;

        if event == "ui_guide" && value > 0.5 {
            return Ok(target);
        }
    }

    Err(message(format!("D-Bus event stream closed: {target}")))
}
