use crate::api::{ApiCommand, SharedStatus};
use crate::error::{message, Result};
use crate::inputplumber::InputPlumber;
use crate::registry::ResolvedLaunch;
use crate::session::{Session, SessionOutcome};
use crate::systemd::UserSystemd;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

pub async fn recover_runtime() -> Result<()> {
    let systemd = UserSystemd::connect().await?;
    if systemd.recover_orphaned_game().await? {
        println!("Stopped orphaned zaman-game.service during recovery.");
    }

    match InputPlumber::connect().await {
        Ok(input) => match input.discover().await {
            Ok(inventory) => {
                input
                    .set_intercept_mode(&inventory, crate::inputplumber::INTERCEPT_NONE)
                    .await?;
            }
            Err(error) => eprintln!("Input recovery deferred: {error}"),
        },
        Err(error) => eprintln!("Input recovery deferred: {error}"),
    }

    Ok(())
}

pub async fn run_worker(
    mut commands: mpsc::Receiver<ApiCommand>,
    status: SharedStatus,
) -> Result<()> {
    let mut sessions: JoinSet<Result<SessionOutcome>> = JoinSet::new();
    let mut active_stop: Option<watch::Sender<bool>> = None;

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(ApiCommand::Launch(launch)) => {
                        if active_stop.is_some() {
                            status.fail("worker received a second launch while active");
                            continue;
                        }
                        let (stop_tx, stop_rx) = watch::channel(false);
                        active_stop = Some(stop_tx);
                        sessions.spawn(run_session(launch, stop_rx));
                    }
                    Some(ApiCommand::Stop) => {
                        if let Some(stop) = &active_stop {
                            status.mark_stopping();
                            let _ = stop.send(true);
                        }
                    }
                    Some(ApiCommand::Shutdown) | None => {
                        if let Some(stop) = &active_stop {
                            status.mark_stopping();
                            let _ = stop.send(true);
                        }
                        while let Some(result) = sessions.join_next().await {
                            record_completion(&status, result);
                        }
                        return Ok(());
                    }
                }
            }
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(result) = completed {
                    record_completion(&status, result);
                }
                active_stop = None;
            }
        }
    }
}

async fn run_session(
    launch: ResolvedLaunch,
    stop: watch::Receiver<bool>,
) -> Result<SessionOutcome> {
    println!(
        "Launching system={} emulator={} ({}) ROM={}",
        launch.system_id, launch.emulator_id, launch.emulator_name, launch.rom_path
    );

    let input = InputPlumber::connect().await?;
    let inventory = input.discover().await?;
    let systemd = UserSystemd::connect().await?;
    println!(
        "Discovered {} composite device(s) and {} normalized D-Bus target(s).",
        inventory.composite_count(),
        inventory.target_count()
    );

    let mut session = Session::new(input, systemd, inventory);
    session.run(&launch.command, stop).await
}

fn record_completion(
    status: &SharedStatus,
    completion: std::result::Result<Result<SessionOutcome>, tokio::task::JoinError>,
) {
    match completion {
        Ok(Ok(outcome)) => {
            println!("Session finished: {outcome}.");
            status.finish(outcome.to_string());
        }
        Ok(Err(error)) => {
            eprintln!("Session failed: {error}");
            status.fail(error.to_string());
        }
        Err(error) => {
            let error = message(format!("session task failed: {error}"));
            eprintln!("{error}");
            status.fail(error.to_string());
        }
    }
}
