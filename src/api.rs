use crate::error::{message, Result};
use crate::menu::{MenuController, MenuEvent};
use crate::registry::{Registry, ResolvedLaunch};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::mpsc;
use zaman_sessiond::contract::{INTERFACE, PATH, VERSION};
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface, Connection};

#[derive(Clone, Debug)]
pub enum ApiCommand {
    Launch(ResolvedLaunch),
    Stop,
    Resume,
    ExitGame,
    Shutdown, // power off the SYSTEM via logind
    Quit,     // shut down the DAEMON only
}

#[derive(Clone, Debug)]
pub struct StatusSnapshot {
    pub state: String,
    pub system_id: String,
    pub emulator_id: String,
    pub rom_path: String,
    pub last_result: String,
    pub last_error: String,
}

#[derive(Debug)]
struct Status {
    state: String,
    system_id: String,
    emulator_id: String,
    rom_path: String,
    last_result: String,
    last_error: String,
}

#[derive(Clone, Debug)]
pub struct SharedStatus {
    inner: Arc<Mutex<Status>>,
}

impl SharedStatus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Status {
                state: "Idle".to_string(),
                system_id: String::new(),
                emulator_id: String::new(),
                rom_path: String::new(),
                last_result: String::new(),
                last_error: String::new(),
            })),
        }
    }

    pub fn reserve(&self, launch: &ResolvedLaunch) -> std::result::Result<(), String> {
        let mut status = self.lock();
        if matches!(status.state.as_str(), "Active" | "Stopping") {
            return Err("a Zaman game session is already active".to_string());
        }

        status.state = "Active".to_string();
        status.system_id.clone_from(&launch.system_id);
        status.emulator_id.clone_from(&launch.emulator_id);
        status.rom_path.clone_from(&launch.rom_path);
        status.last_result.clear();
        status.last_error.clear();
        Ok(())
    }

    pub fn mark_stopping(&self) {
        let mut status = self.lock();
        if status.state == "Active" {
            status.state = "Stopping".to_string();
        }
    }

    pub fn finish(&self, result: impl Into<String>) {
        let mut status = self.lock();
        status.state = "Idle".to_string();
        status.last_result = result.into();
        status.last_error.clear();
    }

    pub fn fail(&self, error: impl Into<String>) {
        let mut status = self.lock();
        status.state = "Failed".to_string();
        status.last_result.clear();
        status.last_error = error.into();
    }

    pub fn snapshot(&self) -> StatusSnapshot {
        let status = self.lock();
        StatusSnapshot {
            state: status.state.clone(),
            system_id: status.system_id.clone(),
            emulator_id: status.emulator_id.clone(),
            rom_path: status.rom_path.clone(),
            last_result: status.last_result.clone(),
            last_error: status.last_error.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Status> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct SessionApi {
    registry: Arc<Registry>,
    status: SharedStatus,
    menu: MenuController,
    commands: mpsc::Sender<ApiCommand>,
}

pub async fn publish_menu_events(
    connection: Connection,
    mut events: mpsc::UnboundedReceiver<MenuEvent>,
) -> Result<()> {
    let signal_emitter = SignalEmitter::new(&connection, PATH)?.into_owned();

    while let Some(event) = events.recv().await {
        match event {
            MenuEvent::Opened(snapshot) => {
                signal_emitter
                    .emit(
                        INTERFACE,
                        "MenuOpened",
                        &(snapshot.generation, snapshot.reason.as_str()),
                    )
                    .await?;
            }
            MenuEvent::Closed(snapshot) => {
                signal_emitter
                    .emit(
                        INTERFACE,
                        "MenuClosed",
                        &(snapshot.generation, snapshot.reason.as_str()),
                    )
                    .await?;
            }
            MenuEvent::Input {
                generation,
                event,
                value,
            } => {
                signal_emitter
                    .emit(INTERFACE, "MenuInput", &(generation, event.as_str(), value))
                    .await?;
            }
        }
    }

    Err(message("menu event publisher stopped"))
}

impl SessionApi {
    pub fn new(
        registry: Arc<Registry>,
        status: SharedStatus,
        menu: MenuController,
        commands: mpsc::Sender<ApiCommand>,
    ) -> Self {
        Self {
            registry,
            status,
            menu,
            commands,
        }
    }

    async fn send_command(&self, command: ApiCommand) -> fdo::Result<()> {
        self.commands
            .send(command)
            .await
            .map_err(|_| fdo::Error::Failed("zaman-sessiond worker is unavailable".to_string()))
    }

    fn require_open_menu(&self) -> fdo::Result<()> {
        if self.menu.snapshot().open {
            Ok(())
        } else {
            Err(fdo::Error::Failed(
                "the Zaman system menu is not open".to_string(),
            ))
        }
    }
}

#[interface(name = "com.kawnelectro.Zaman.Session1")]
impl SessionApi {
    async fn launch(&self, system_id: &str, rom_path: &str) -> fdo::Result<()> {
        let launch = self
            .registry
            .resolve(system_id, rom_path)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        self.status.reserve(&launch).map_err(fdo::Error::Failed)?;

        if self
            .commands
            .send(ApiCommand::Launch(launch))
            .await
            .is_err()
        {
            self.status.fail("zaman-sessiond worker is unavailable");
            return Err(fdo::Error::Failed(
                "zaman-sessiond worker is unavailable".to_string(),
            ));
        }
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.status.mark_stopping();
        self.send_command(ApiCommand::Stop).await
    }

    async fn resume(&self) -> fdo::Result<()> {
        self.require_open_menu()?;
        self.send_command(ApiCommand::Resume).await
    }

    async fn exit_game(&self) -> fdo::Result<()> {
        self.require_open_menu()?;
        self.status.mark_stopping();
        self.send_command(ApiCommand::ExitGame).await
    }

    async fn shutdown(&self) -> fdo::Result<()> {
        // No require_open_menu(): shutdown must also work from the
        // frontend when no game is running (idle power-off).
        self.status.mark_stopping();
        self.send_command(ApiCommand::Shutdown).await
    }

    #[zbus(out_args(
        "state",
        "system_id",
        "emulator_id",
        "rom_path",
        "last_result",
        "last_error"
    ))]
    fn status(&self) -> (String, String, String, String, String, String) {
        let status = self.status.snapshot();
        (
            status.state,
            status.system_id,
            status.emulator_id,
            status.rom_path,
            status.last_result,
            status.last_error,
        )
    }

    fn version(&self) -> &str {
        VERSION
    }

    #[zbus(out_args("open", "generation", "reason"))]
    fn menu_status(&self) -> (bool, u64, String) {
        let menu = self.menu.snapshot();
        (menu.open, menu.generation, menu.reason)
    }

    #[zbus(signal)]
    async fn menu_opened(
        signal_emitter: &SignalEmitter<'_>,
        generation: u64,
        reason: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn menu_closed(
        signal_emitter: &SignalEmitter<'_>,
        generation: u64,
        reason: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn menu_input(
        signal_emitter: &SignalEmitter<'_>,
        generation: u64,
        event: &str,
        value: f64,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::SharedStatus;
    use crate::command::CommandSpec;
    use crate::registry::ResolvedLaunch;
    use std::collections::BTreeMap;

    fn launch() -> ResolvedLaunch {
        let executable = std::env::current_exe()
            .expect("test executable path")
            .to_string_lossy()
            .into_owned();
        ResolvedLaunch {
            system_id: "nes".to_string(),
            emulator_id: "test".to_string(),
            emulator_name: "Test".to_string(),
            rom_path: "/tmp/test.nes".to_string(),
            command: CommandSpec::new(vec![executable], BTreeMap::new(), None).expect("command"),
        }
    }

    #[test]
    fn reservation_is_atomic_and_reusable_after_failure() {
        let status = SharedStatus::new();
        let launch = launch();

        status.reserve(&launch).expect("first reservation");
        assert!(status.reserve(&launch).is_err());
        status.fail("test failure");
        status.reserve(&launch).expect("retry after failure");
    }
}
