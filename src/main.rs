mod api;
mod command;
mod daemon;
mod error;
mod guide;
mod inputplumber;
mod menu;
mod operations;
mod registry;
mod session;
mod systemd;

use crate::api::{publish_menu_events, ApiCommand, SessionApi, SharedStatus};
use crate::daemon::{monitor_menu_client, recover_runtime, run_worker};
use crate::error::Result;
use crate::menu::MenuController;
use crate::registry::Registry;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use zaman_sessiond::contract::{PATH, SERVICE, VERSION};
use zbus::connection::Builder;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let registry = Arc::new(Registry::load()?);
    recover_runtime().await?;

    let status = SharedStatus::new();
    let (menu, menu_events) = MenuController::new();
    let (commands, receiver) = mpsc::channel(8);
    let api = SessionApi::new(registry, status.clone(), menu.clone(), commands.clone());
    let connection = Builder::session()?
        .serve_at(PATH, api)?
        .name(SERVICE)?
        .build()
        .await?;

    println!("zaman-sessiond {VERSION} ready on {SERVICE} {PATH}");

    let worker = run_worker(receiver, status, menu);
    let menu_publisher = publish_menu_events(connection.clone(), menu_events);
    let menu_client_monitor = monitor_menu_client(connection.clone(), commands.clone());
    tokio::pin!(worker);
    tokio::pin!(menu_publisher);
    tokio::pin!(menu_client_monitor);

    let trigger_result = tokio::select! {
        result = &mut worker => return result,
        result = &mut menu_publisher => result,
        result = &mut menu_client_monitor => result,
        result = wait_for_shutdown_signal() => result,
    };

    println!("zaman-sessiond shutting down.");
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
