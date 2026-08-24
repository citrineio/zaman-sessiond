mod api;
mod command;
mod contract;
mod daemon;
mod error;
mod guide;
mod inputplumber;
mod registry;
mod session;
mod systemd;

use crate::api::{ApiCommand, SessionApi, SharedStatus};
use crate::contract::{PATH, SERVICE, VERSION};
use crate::daemon::{recover_runtime, run_worker};
use crate::error::Result;
use crate::registry::Registry;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use zbus::connection::Builder;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let registry = Arc::new(Registry::load()?);
    recover_runtime().await?;

    let status = SharedStatus::new();
    let (commands, receiver) = mpsc::channel(8);
    let api = SessionApi::new(registry, status.clone(), commands.clone());
    let _connection = Builder::session()?
        .serve_at(PATH, api)?
        .name(SERVICE)?
        .build()
        .await?;

    println!("zaman-sessiond {VERSION} ready on {SERVICE} {PATH}");

    let worker = run_worker(receiver, status);
    tokio::pin!(worker);

    tokio::select! {
        result = &mut worker => return result,
        result = wait_for_shutdown_signal() => result?,
    }

    println!("zaman-sessiond shutting down.");
    let _ = commands.send(ApiCommand::Shutdown).await;
    worker.await
}

async fn wait_for_shutdown_signal() -> Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        result = tokio::signal::ctrl_c() => result?,
        _ = terminate.recv() => {}
    }

    Ok(())
}
