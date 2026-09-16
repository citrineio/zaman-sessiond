use crate::command::CommandSpec;
use crate::error::{message, Result};
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use zbus::proxy::SignalStream;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, Proxy};

const SERVICE: &str = "org.freedesktop.systemd1";
const PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const GAME_UNIT: &str = "zaman-game.service";
const MENU_UNIT: &str = "zaman-menu.service";
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameHandle {
    unit: String,
}

impl GameHandle {
    pub fn unit(&self) -> &str {
        &self.unit
    }
}

pub struct UserSystemd {
    connection: Connection,
}

impl UserSystemd {
    pub async fn connect() -> Result<Self> {
        let connection = Connection::session().await?;
        let supervisor = Self { connection };
        supervisor.subscribe().await?;
        Ok(supervisor)
    }

    async fn manager(&self) -> Result<Proxy<'_>> {
        Ok(Proxy::new(&self.connection, SERVICE, PATH, MANAGER_INTERFACE).await?)
    }

    async fn subscribe(&self) -> Result<()> {
        let manager = self.manager().await?;
        let _: () = manager.call("Subscribe", &()).await?;
        Ok(())
    }

    pub async fn recover_orphaned_game(&self) -> Result<bool> {
        let manager = self.manager().await?;
        let loaded: zbus::Result<OwnedObjectPath> = manager.call("GetUnit", &(GAME_UNIT,)).await;
        if loaded.is_err() {
            return Ok(false);
        }

        self.stop(&GameHandle {
            unit: GAME_UNIT.to_string(),
        })
        .await?;
        Ok(true)
    }

    pub async fn start_menu(&self) -> Result<()> {
        let manager = self.manager().await?;
        let mut removed_jobs = manager.receive_signal("JobRemoved").await?;
        let job_path: OwnedObjectPath = manager.call("StartUnit", &(MENU_UNIT, "replace")).await?;
        wait_for_job(&mut removed_jobs, &job_path, MENU_UNIT, "start").await
    }

    pub async fn launch(&self, command: &CommandSpec) -> Result<GameHandle> {
        let manager = self.manager().await?;

        let loaded: zbus::Result<OwnedObjectPath> = manager.call("GetUnit", &(GAME_UNIT,)).await;
        if loaded.is_ok() {
            return Err(message(format!(
                "another Zaman game session already owns {GAME_UNIT}"
            )));
        }

        let mut removed_jobs = manager.receive_signal("JobRemoved").await?;
        let argv: Vec<&str> = command.argv().iter().map(String::as_str).collect();
        let exec_start = vec![(command.executable(), argv, false)];
        let mut properties = vec![
            ("Description", Value::new("Zaman supervised game session")),
            ("Type", Value::new("exec")),
            ("ExecStart", Value::new(exec_start)),
            ("KillMode", Value::new("control-group")),
            ("KillSignal", Value::new(2_i32)), // SIGINT: Mesen graceful shutdown
            ("TimeoutStopUSec", Value::new(3_000_000_u64)),
            ("SendSIGKILL", Value::new(true)),
            ("CollectMode", Value::new("inactive-or-failed")),
        ];
        if !command.environment().is_empty() {
            properties.push(("Environment", Value::new(command.environment().to_vec())));
        }
        if let Some(directory) = command.working_directory() {
            properties.push(("WorkingDirectory", Value::new(directory)));
        }
        let auxiliary_units: Vec<(&str, Vec<(&str, Value<'_>)>)> = Vec::new();

        let job_path: OwnedObjectPath = manager
            .call(
                "StartTransientUnit",
                &(GAME_UNIT, "fail", properties, auxiliary_units),
            )
            .await?;

        println!("Queued systemd start job {job_path} for {GAME_UNIT}.");
        wait_for_job(&mut removed_jobs, &job_path, GAME_UNIT, "start").await?;
        println!("systemd confirmed {GAME_UNIT} started.");

        Ok(GameHandle {
            unit: GAME_UNIT.to_string(),
        })
    }

    pub async fn wait_for_exit(&self, game: &GameHandle) -> Result<()> {
        let manager = self.manager().await?;
        let mut removed_units = manager
            .receive_signal_with_args("UnitRemoved", &[(0, game.unit())])
            .await?;

        // Register the signal match before checking the unit. This closes the
        // race where a short-lived emulator exits immediately after launch.
        let loaded: zbus::Result<OwnedObjectPath> = manager.call("GetUnit", &(game.unit(),)).await;
        if loaded.is_err() {
            return Ok(());
        }

        while let Some(message) = removed_units.next().await {
            let (unit, _path): (String, OwnedObjectPath) = message.body().deserialize()?;
            if unit == game.unit() {
                println!("systemd reported {} exited.", game.unit());
                return Ok(());
            }
        }

        Err(message("systemd UnitRemoved stream closed"))
    }

    pub async fn stop(&self, game: &GameHandle) -> Result<()> {
        let manager = self.manager().await?;
        let loaded: zbus::Result<OwnedObjectPath> = manager.call("GetUnit", &(game.unit(),)).await;
        if loaded.is_err() {
            return Ok(());
        }

        let mut removed_jobs = manager.receive_signal("JobRemoved").await?;
        let stop_result: zbus::Result<OwnedObjectPath> =
            manager.call("StopUnit", &(game.unit(), "replace")).await;

        let job_path = match stop_result {
            Ok(path) => path,
            Err(stop_error) => {
                let still_loaded: zbus::Result<OwnedObjectPath> =
                    manager.call("GetUnit", &(game.unit(),)).await;
                if still_loaded.is_err() {
                    return Ok(());
                }
                return Err(stop_error.into());
            }
        };

        println!("Queued systemd stop job {job_path} for {}.", game.unit());
        wait_for_job(&mut removed_jobs, &job_path, game.unit(), "stop").await?;
        println!("systemd confirmed {} stopped.", game.unit());

        Ok(())
    }
}

async fn wait_for_job(
    removed_jobs: &mut SignalStream<'_>,
    expected_path: &OwnedObjectPath,
    expected_unit: &str,
    operation: &str,
) -> Result<()> {
    let wait = async {
        while let Some(signal) = removed_jobs.next().await {
            let (_id, path, unit, result): (u32, OwnedObjectPath, String, String) =
                signal.body().deserialize()?;

            if &path != expected_path {
                continue;
            }

            if unit != expected_unit || result != "done" {
                return Err(message(format!(
                    "systemd {operation} job failed: unit={unit}, result={result}"
                )));
            }

            return Ok(());
        }

        Err(message("systemd JobRemoved stream closed"))
    };

    timeout(JOB_TIMEOUT, wait).await.map_err(|_| {
        message(format!(
            "systemd {operation} job timed out for {expected_unit}"
        ))
    })?
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::Value;

    #[test]
    fn exec_start_uses_systemd_argv_signature() {
        let exec_start = vec![(
            "/usr/bin/emulator",
            vec!["/usr/bin/emulator", "ROM with spaces.nes"],
            false,
        )];
        let value = Value::new(exec_start);

        assert_eq!(value.value_signature().to_string(), "a(sasb)");
    }
}
