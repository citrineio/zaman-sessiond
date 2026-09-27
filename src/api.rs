use crate::error::{message, Result};
use crate::menu::{MenuController, MenuEvent};
use crate::registry::{Registry, ResolvedLaunch};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{mpsc, oneshot};
use zaman_sessiond::contract::{MenuContextTuple, INTERFACE, PATH, VERSION};
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface, Connection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Open,
    Close,
    Toggle,
}

#[derive(Debug)]
pub enum ApiCommand {
    Guarded { command: Box<ApiCommand>, reply: oneshot::Sender<std::result::Result<(), String>> },
    StartTransfer { menu_generation: u64, reply: oneshot::Sender<std::result::Result<u64, String>> },
    Launch(ResolvedLaunch),
    Stop,
    Menu {
        action: MenuAction,
        reason: &'static str,
        reply: oneshot::Sender<std::result::Result<(), String>>,
    },
    MenuClientExited,
    ExitGame,
    Reboot,   // reboot the SYSTEM after graceful game stop
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
            MenuEvent::ContextChanged(generation) => {
                signal_emitter.emit(INTERFACE, "MenuContextChanged", &(generation,)).await?;
            }
            MenuEvent::TransferInput { generation, event, value } => {
                signal_emitter.emit(INTERFACE, "TransferInput", &(generation, event, value)).await?;
            }
            MenuEvent::Opened(snapshot) => {
                signal_emitter
                    .emit(
                        INTERFACE,
                        "MenuOpened",
                        &(snapshot.generation, snapshot.reason.as_str()),
                    )
                    .await?;
                signal_emitter
                    .emit(INTERFACE, "MenuContextChanged", &(snapshot.generation,))
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
                signal_emitter
                    .emit(INTERFACE, "MenuContextChanged", &(snapshot.generation,))
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

    fn require_transfer_idle(&self) -> fdo::Result<()> {
        if self.menu.transfer_busy() {
            Err(fdo::Error::Failed("busy: close Transfer Games first".into()))
        } else { Ok(()) }
    }

    async fn send_command(&self, command: ApiCommand) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        let (reply, response) = oneshot::channel();
        self.commands
            .send(ApiCommand::Guarded { command: Box::new(command), reply })
            .await
            .map_err(|_| fdo::Error::Failed("zaman-sessiond worker is unavailable".to_string()))?;
        response.await.map_err(|_| fdo::Error::Failed("zaman-sessiond worker stopped".into()))?
            .map_err(fdo::Error::Failed)
    }

    async fn request_menu(&self, action: MenuAction, reason: &'static str) -> fdo::Result<()> {
        let (reply, response) = oneshot::channel();
        self.send_command(ApiCommand::Menu {
            action,
            reason,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| fdo::Error::Failed("zaman-sessiond menu worker stopped".to_string()))?
            .map_err(fdo::Error::Failed)
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
    #[zbus(out_args("generation"))]
    async fn start_transfer(&self, menu_generation: u64) -> fdo::Result<u64> {
        let (reply, response) = oneshot::channel();
        self.send_command(ApiCommand::StartTransfer { menu_generation, reply }).await?;
        response.await.map_err(|_| fdo::Error::Failed("transfer worker stopped".into()))?
            .map_err(fdo::Error::Failed)
    }

    #[zbus(out_args("generation", "phase"))]
    fn transfer_context(&self) -> (u64, String) { self.menu.transfer_context() }

    async fn transfer_presented(
        &self, generation: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<()> {
        let generation = parse_transfer_generation(generation)?;
        let sender = header.sender().ok_or_else(|| fdo::Error::AccessDenied("missing sender".into()))?;
        let bus = zbus::Proxy::new(connection, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus")
            .await.map_err(|error| fdo::Error::Failed(error.to_string()))?;
        let owner: String = bus.call("GetNameOwner", &(zaman_sessiond::contract::TRANSFER_SERVICE,))
            .await.map_err(|error| fdo::Error::Failed(error.to_string()))?;
        if sender.as_str() != owner || !self.menu.transfer_presented(generation, &owner) {
            return Err(fdo::Error::AccessDenied("stale generation or unregistered transfer owner".into()));
        }
        Ok(())
    }

    #[zbus(signal)]
    async fn transfer_input(signal_emitter: &SignalEmitter<'_>, generation: u64, event: &str, value: f64) -> zbus::Result<()>;

    async fn launch(&self, system_id: &str, rom_path: &str) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        if self.menu.snapshot().open {
            return Err(fdo::Error::Failed(
                "close the Zaman menu before launching a game".to_string(),
            ));
        }
        let launch = self
            .registry
            .resolve(system_id, rom_path)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        self.status.reserve(&launch).map_err(fdo::Error::Failed)?;

        if let Err(error) = self.send_command(ApiCommand::Launch(launch)).await {
            self.status.fail(error.to_string());
            return Err(error);
        }
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        self.status.mark_stopping();
        self.send_command(ApiCommand::Stop).await
    }

    async fn resume(&self) -> fdo::Result<()> {
        self.request_menu(MenuAction::Close, "resume").await
    }

    async fn open_menu(&self) -> fdo::Result<()> {
        self.request_menu(MenuAction::Open, "manual-open").await
    }

    async fn close_menu(&self) -> fdo::Result<()> {
        self.request_menu(MenuAction::Close, "manual-close").await
    }

    async fn toggle_menu(&self) -> fdo::Result<()> {
        self.request_menu(MenuAction::Toggle, "manual-toggle").await
    }

    async fn exit_game(&self) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        self.require_open_menu()?;
        self.status.mark_stopping();
        self.send_command(ApiCommand::ExitGame).await
    }

    async fn reboot(&self) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        self.status.mark_stopping();
        if let Err(error) = self.send_command(ApiCommand::Reboot).await {
            self.status.fail(format!("reboot request failed: {error}"));
            return Err(error);
        }
        Ok(())
    }

    async fn shutdown(&self) -> fdo::Result<()> {
        self.require_transfer_idle()?;
        // No require_open_menu(): shutdown must also work from the
        // frontend when no game is running (idle power-off).
        self.status.mark_stopping();
        self.send_command(ApiCommand::Shutdown).await
    }

    #[zbus(out_args(
        "open",
        "generation",
        "return_target",
        "game_state",
        "pending",
        "error",
        "allowed_actions"
    ))]
    async fn menu_context(&self) -> fdo::Result<MenuContextTuple> {
        Ok(self.menu.context())
    }

    #[zbus(out_args("game_state"))]
    async fn game_state(&self) -> fdo::Result<String> {
        Ok(self.menu.context().3)
    }

    async fn request_menu_action(&self, generation: u64, action: String) -> fdo::Result<()> {
        let context = self.menu.context();
        validate_menu_action(&context, generation, &action)?;
        match action.as_str() {
            "transfer" => self.start_transfer(generation).await.map(|_| ()),
            "resume" => self.resume().await,
            "exit-game" => self.exit_game().await,
            "reboot" => self.reboot().await,
            "shutdown" => self.shutdown().await,
            other => Err(fdo::Error::Failed(format!(
                "unknown menu action '{}'",
                other
            ))),
        }
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

    #[zbus(out_args("foreground", "return_target", "reason"))]
    fn foreground_status(&self) -> (String, String, String) {
        let menu = self.menu.snapshot();
        (
            menu.foreground.as_str().to_string(),
            menu.return_target
                .map(|target| target.as_str().to_string())
                .unwrap_or_default(),
            menu.reason,
        )
    }

    fn menu_presented(&self, generation: u64) -> fdo::Result<()> {
        if self.menu.presented(generation) {
            Ok(())
        } else {
            Err(fdo::Error::Failed(format!(
                "menu generation {generation} is no longer current"
            )))
        }
    }

    #[zbus(signal)]
    async fn menu_context_changed(
        signal_emitter: &SignalEmitter<'_>,
        generation: u64,
    ) -> zbus::Result<()>;

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

// Qt's generic D-Bus call binds Python ints as signed values. A decimal
// string on this one inbound method preserves every u64 generation exactly.
fn parse_transfer_generation(value: &str) -> fdo::Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(fdo::Error::InvalidArgs("generation must be an unsigned decimal integer".into()));
    }
    value.parse::<u64>().map_err(|_| fdo::Error::InvalidArgs("generation is outside the u64 range".into()))
}

fn validate_menu_action(
    context: &MenuContextTuple,
    generation: u64,
    action: &str,
) -> fdo::Result<()> {
    if context.4 { return Err(fdo::Error::Failed("busy: close Transfer Games first".into())); }
    if context.1 != generation {
        return Err(fdo::Error::Failed(format!(
            "stale menu generation {} (current {})",
            generation, context.1
        )));
    }
    if !context.6.iter().any(|allowed| allowed == action) {
        return Err(fdo::Error::Failed(format!(
            "action '{}' is not allowed in this context",
            action
        )));
    }
    Ok(())
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

    #[test]
    fn reboot_guard_accepts_current_open_context_only() {
        let (menu, _) = crate::menu::MenuController::new();
        assert!(super::validate_menu_action(&menu.context(), 0, "reboot").is_err());
        for game in [false, true] {
            if game {
                menu.game_started();
            }
            menu.open("test");
            let context = menu.context();
            assert!(super::validate_menu_action(&context, context.1, "reboot").is_ok());
            assert!(super::validate_menu_action(&context, context.1 - 1, "reboot").is_err());
            assert!(super::validate_menu_action(&context, context.1, "unsupported").is_err());
            let mut omitted = context.clone();
            omitted.6.retain(|action| action != "reboot");
            assert!(super::validate_menu_action(&omitted, context.1, "reboot").is_err());
            menu.close("test", game);
            assert!(super::validate_menu_action(&menu.context(), context.1, "reboot").is_err());
        }
    }
    #[test]
    fn transfer_qt_decimal_generation_preserves_full_u64_and_rejects_invalid_input() {
        assert_eq!(super::parse_transfer_generation("18446744073709551615").unwrap(), u64::MAX);
        for invalid in ["", "-1", "+1", "1.0", "18446744073709551616"] {
            assert!(super::parse_transfer_generation(invalid).is_err());
        }
    }

}
