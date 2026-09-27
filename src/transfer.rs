//! One foreground lease, one supervised process, and event-driven readiness.
use crate::error::{message, Result};
use crate::menu::MenuController;
use crate::systemd::UserSystemd;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::watch;
use zaman_sessiond::contract::TRANSFER_SERVICE;
use zbus::{Connection, Proxy};

pub const UNIT: &str = "zaman-transfer-gui.service";
const PRESENT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Completion {
    pub generation: u64,
    pub error: Option<String>,
    pub cleaned: bool,
}

pub async fn run(menu: MenuController, generation: u64, mut stop: watch::Receiver<bool>, input: crate::inputplumber::InputPlumber, inventory: crate::inputplumber::Inventory) -> Completion {
    let mut readiness = menu.transfer_changes();
    let stopped = *stop.borrow();
    let mut systemd = None;
    let result = if stopped { Ok(()) } else {
        // Keep the manager connection for cleanup. Cancelling the start future
        // before StopUnit and using the same connection preserves message order.
        let running = async {
            systemd = Some(UserSystemd::connect().await?);
            input.set_intercept_mode(&inventory, crate::inputplumber::INTERCEPT_ALL).await?;
            supervise(&menu, generation, systemd.as_ref().unwrap()).await
        };
        tokio::pin!(running);
        let deadline = tokio::time::sleep(PRESENT_TIMEOUT);
        tokio::pin!(deadline);
        let mut ready = false;
        loop {
            tokio::select! {
                result = &mut running => break result,
                _ = stop.changed() => break Ok(()),
                _ = &mut deadline, if !ready => break Err(message("GUI did not present within 10 seconds")),
                change = readiness.changed() => {
                    if change.is_err() { break Err(message("readiness channel closed")); }
                    let context = menu.transfer_context();
                    ready = context.0 == generation && context.1 == "active";
                }
            }
        }
    };
    menu.stop_transfer(generation);
    // Always stop the attempted unit, even if it never acquired its bus name.
    // StopUnit replaces an outstanding StartUnit job before releasing the lease.
    let cleanup = match systemd {
        Some(systemd) => systemd.stop_transfer().await,
        None => async { UserSystemd::connect().await?.stop_transfer().await }.await,
    };
    let cleaned = cleanup.is_ok();
    let error = match (result, cleanup) {
        (_, Err(error)) => Some(format!("Transfer cleanup failed: {error}. Restart the session before retrying.")),
        (Err(error), _) => Some(format!("Transfer Games could not stay open: {error}. Check the GUI installation and try again.")),
        _ => None,
    };
    Completion { generation, error, cleaned }
}

async fn supervise(menu: &MenuController, generation: u64, systemd: &UserSystemd) -> Result<()> {
    let connection = Connection::session().await?;
    let bus = Proxy::new(&connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").await?;
    let mut owners = bus.receive_signal_with_args("NameOwnerChanged", &[(0, TRANSFER_SERVICE)]).await?;
    let owned: bool = bus.call("NameHasOwner", &(TRANSFER_SERVICE,)).await?;
    if owned { return Err(message("a transfer GUI already owns the bus name")); }
    let manager = Proxy::new(&connection, "org.freedesktop.systemd1", "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager").await?;
    let path: zbus::zvariant::OwnedObjectPath = manager.call("LoadUnit", &(UNIT,)).await?;
    let unit = Proxy::new(&connection, "org.freedesktop.systemd1", path.as_str(), "org.freedesktop.systemd1.Unit").await?;
    let mut states = unit.receive_property_changed::<String>("ActiveState").await;
    let mut readiness = menu.transfer_changes();
    let start = systemd.start_transfer();
    tokio::pin!(start);
    let mut started = false;
    let mut ready = false;
    let mut observed_owner: Option<String> = None;
    loop {
        tokio::select! {
            result = &mut start, if !started => {
                result?;
                started = true;
                let state: String = unit.get_property("ActiveState").await?;
                if matches!(state.as_str(), "inactive" | "failed" | "deactivating") {
                    return Err(message("GUI service exited before presenting"));
                }
            }
            change = readiness.changed() => {
                change.map_err(|_| message("readiness channel closed"))?;
                let context = menu.transfer_context();
                if context.0 != generation { return Err(message("transfer generation superseded")); }
                ready = context.1 == "active";
                if ready {
                    let owner: String = bus.call("GetNameOwner", &(TRANSFER_SERVICE,)).await?;
                    if menu.transfer_owner().as_deref() != Some(owner.as_str()) {
                        return Err(message("presenting GUI owner disappeared"));
                    }
                    observed_owner = Some(owner);
                }
            }
            change = owners.next() => {
                let change = change.ok_or_else(|| message("transfer owner stream closed"))?;
                let (_, old, new): (String, String, String) = change.body().deserialize()?;
                if !old.is_empty() && (observed_owner.as_deref() == Some(old.as_str()) || menu.transfer_owner().as_deref() == Some(old.as_str())) {
                    return if ready { Ok(()) } else { Err(message("GUI exited during startup")) };
                }
                if !new.is_empty() { observed_owner = Some(new); }
            }
            change = states.next() => {
                let change = change.ok_or_else(|| message("transfer service stream closed"))?;
                let state = change.get().await?;
                if matches!(state.as_str(), "failed" | "deactivating") || (started && state == "inactive") {
                    return if ready && state != "failed" { Ok(()) } else { Err(message("GUI service exited")) };
                }
            }
        }
    }
}
