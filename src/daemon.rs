use crate::api::{ApiCommand, SharedStatus};
use crate::error::{message, Result};
use crate::inputplumber::InputPlumber;
use crate::menu::MenuController;
use crate::registry::ResolvedLaunch;
use crate::session::{Session, SessionControl, SessionOutcome};
use crate::systemd::UserSystemd;
use tokio::sync::mpsc;
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
    menu: MenuController,
) -> Result<()> {
    let mut sessions: JoinSet<Result<SessionOutcome>> = JoinSet::new();
    let mut active_control: Option<mpsc::Sender<SessionControl>> = None;

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(ApiCommand::Launch(launch)) => {
                        if active_control.is_some() {
                            status.fail("worker received a second launch while active");
                            continue;
                        }
                        let (control, controls) = mpsc::channel(8);
                        active_control = Some(control);
                        sessions.spawn(run_session(launch, controls, menu.clone()));
                    }
                    Some(ApiCommand::Stop) => {
                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::Stop).await;
                        }
                    }
                    Some(ApiCommand::Resume) => {
                        if let Some(control) = &active_control {
                            let _ = control.send(SessionControl::Resume).await;
                        }
                    }
                    Some(ApiCommand::ExitGame) => {
                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::ExitGame).await;
                        }
                    }
                    Some(ApiCommand::Shutdown) => {
                        match &active_control {
                            // Game running: graceful exit first; PowerOff
                            // fires in the completion branch below once the
                            // session reports ShutdownRequested.
                            Some(control) => {
                                status.mark_stopping();
                                let _ = control.send(SessionControl::Shutdown).await;
                            }
                            // Idle: nothing to save, power off immediately.
                            None => power_off().await,
                        }
                    }
                    Some(ApiCommand::Quit) | None => {
                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::Stop).await;
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
                    let shutdown = matches!(result, Ok(Ok(SessionOutcome::ShutdownRequested)));
                    record_completion(&status, result);
                    active_control = None;
                    if shutdown {
                        power_off().await;
                    }
                }
            }
        }
    }
}

async fn run_session(
    launch: ResolvedLaunch,
    controls: mpsc::Receiver<SessionControl>,
    menu: MenuController,
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

    let mut session = Session::new(input, systemd, inventory, menu);
    session.run(&launch.command, controls).await
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

/// Powers the system off through logind so polkit policy applies and
/// systemd performs the full unmount+sync teardown. Failure is logged and
/// the daemon keeps running — the game is already safely exited by then.
async fn power_off() {
    async fn inner() -> zbus::Result<()> {
        let connection = zbus::Connection::system().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        proxy.call::<_, _, ()>("PowerOff", &(true)).await
    }

    if let Err(error) = inner().await {
        eprintln!("System power-off failed: {error}");
    }
}
