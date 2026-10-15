mod api;
mod command;
mod daemon;
mod error;
mod guide;
mod inputplumber;
mod installed;
mod menu;
mod operations;
mod quickshell_menu;
mod registry;
mod session;
mod systemd;
mod sway;
mod storefront;
mod transfer;

use crate::api::{publish_menu_events, ApiCommand, SessionApi, SharedStatus};
use crate::daemon::{recover_runtime, run_worker};
use crate::error::Result;
use crate::menu::MenuController;
use crate::registry::Registry;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use zaman_sessiond::contract::{PATH, SERVICE, VERSION};
use zaman_sessiond::display::DisplayManager;
use zbus::connection::Builder;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let registry = Arc::new(Registry::load()?);
    recover_runtime().await?;

    let status = SharedStatus::new();
    let (menu, menu_events) = MenuController::new();
    let display_menu = menu.clone();
    let display = tokio::task::spawn_blocking(move || DisplayManager::new(move |generation| {
        let context = display_menu.context();
        context.0 && context.1 == generation && !context.4
    })).await?;
    let (commands, receiver) = mpsc::channel(8);
    let api = SessionApi::new(registry, status.clone(), menu.clone(), commands.clone(), display.clone());
    let connection = Builder::session()?
        .serve_at(PATH, api)?
        .name(SERVICE)?
        .build()
        .await?;

    println!("zaman-sessiond {VERSION} ready on {SERVICE} {PATH}");

    // Repair the Pegasus collection after external Flatpak changes or a prior
    // interrupted transaction. This is best-effort and cannot delay the daemon.
    let _library_sync = tokio::task::spawn_blocking(|| {
        let helper = std::env::var_os("ZAMAN_PEGASUS_LIBRARY_SYNC_HELPER")
            .unwrap_or_else(|| zaman_sessiond::PEGASUS_LIBRARY_SYNC_HELPER.into());
        match std::process::Command::new("/usr/bin/python3").arg(helper).arg("sync").output() {
            Ok(output) if output.status.success() => {},
            Ok(output) => eprintln!("Pegasus library sync failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()),
            Err(error) => eprintln!("Pegasus library sync could not start: {error}"),
        }
    });

    let worker = run_worker(receiver, status, menu);
    let menu_publisher = publish_menu_events(connection.clone(), menu_events);
    tokio::pin!(worker);
    tokio::pin!(menu_publisher);

    let trigger_result = tokio::select! {
        result = &mut worker => {
            let _ = tokio::task::spawn_blocking(move || display.shutdown()).await;
            return result;
        },
        result = &mut menu_publisher => result,
        result = wait_for_shutdown_signal() => result,
    };

    println!("zaman-sessiond shutting down.");
    if let Err(error) = tokio::task::spawn_blocking(move || display.shutdown()).await? {
        eprintln!("Display rollback on shutdown: {error}");
    }
    let _ = commands.send(ApiCommand::Quit).await;
    let worker_result = worker.await;
    trigger_result?;
    worker_result
}

async fn wait_for_shutdown_signal() -> Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        result = tokio::signal::ctrl_c() => result?,
        _ = terminate.recv() => {}
    }

    Ok(())
}
