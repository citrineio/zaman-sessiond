use crate::api::{ApiCommand, MenuAction, SharedStatus};
use crate::error::{message, Result};
use crate::guide::{GuideAction, GuideButton, GuideEffect};
use crate::inputplumber::{
    InputPlumber, Inventory, SystemInputEvent, SystemInputMonitor, INTERCEPT_ALL, INTERCEPT_NONE,
    INTERCEPT_PASS,
};
use crate::menu::MenuController;
use crate::operations::{OperationIdentity, OperationKind, OperationSlot, StartRejected};
use crate::registry::ResolvedLaunch;
use crate::session::{Session, SessionControl, SessionOutcome};
use crate::systemd::UserSystemd;
use std::collections::BTreeMap;
use std::future::pending;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{interval_at, sleep, Instant, MissedTickBehavior};
use zaman_sessiond::contract::MENU_SERVICE;
use zbus::{Connection, Proxy};

const MENU_PRESENT_TIMEOUT: Duration = Duration::from_secs(3);
const INPUT_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const LAUNCH_BUDGET: Duration = Duration::from_secs(40);
const MENU_BUDGET: Duration = Duration::from_secs(3);
const STOP_BUDGET: Duration = Duration::from_secs(40);
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);
const SLOT_TICK: Duration = Duration::from_millis(50);

struct InputRefresh {
    replace_monitor: bool,
    monitor: Option<SystemInputMonitor>,
}

impl InputRefresh {
    fn unchanged() -> Self {
        Self {
            replace_monitor: false,
            monitor: None,
        }
    }
}

struct SystemInput {
    input: InputPlumber,
    inventory: Inventory,
    initialized: bool,
    refresh_error: Option<String>,
}

impl SystemInput {
    async fn connect() -> Result<(Self, Option<SystemInputMonitor>)> {
        let input = InputPlumber::connect().await?;
        let mut system_input = Self {
            input,
            inventory: Inventory::default(),
            initialized: false,
            refresh_error: None,
        };
        let refresh = system_input.refresh(INTERCEPT_PASS, true).await;
        Ok((system_input, refresh.monitor))
    }

    async fn set_mode(&self, mode: u32) -> Result<()> {
        self.input.set_intercept_mode(&self.inventory, mode).await
    }

    async fn refresh(&mut self, mode: u32, force_monitor: bool) -> InputRefresh {
        let inventory = self.input.discover().await;
        self.finish_discovery(inventory, mode, force_monitor).await
    }

    async fn finish_discovery(
        &mut self,
        inventory: Result<Inventory>,
        mode: u32,
        force_monitor: bool,
    ) -> InputRefresh {
        let result = match inventory {
            Ok(inventory) => self.apply_inventory(inventory, mode, force_monitor).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(refresh) => {
                if self.refresh_error.take().is_some() {
                    println!("InputPlumber supervision recovered.");
                }
                refresh
            }
            Err(error) => {
                let error = error.to_string();
                if self.refresh_error.as_deref() != Some(error.as_str()) {
                    eprintln!("InputPlumber refresh failed: {error}; retrying.");
                    self.refresh_error = Some(error);
                }
                InputRefresh::unchanged()
            }
        }
    }

    async fn apply_inventory(
        &mut self,
        inventory: Inventory,
        mode: u32,
        force_monitor: bool,
    ) -> Result<InputRefresh> {
        let changed = !self.initialized || inventory != self.inventory;

        if !should_rebuild_input(
            self.initialized,
            changed,
            force_monitor,
            inventory.has_system_input(),
        ) {
            return Ok(InputRefresh::unchanged());
        }

        if inventory.has_system_input() {
            self.input.set_intercept_mode(&inventory, mode).await?;
        }

        if changed {
            if inventory.has_system_input() {
                println!("InputPlumber system input available; rebuilding supervision.");
            } else {
                eprintln!(
                    "InputPlumber normalized system input is unavailable; retrying discovery."
                );
            }
            inventory.log();
        }

        let monitor = self.input.monitor_system_input(&inventory, changed);
        self.inventory = inventory;
        self.initialized = true;
        Ok(InputRefresh {
            replace_monitor: true,
            monitor,
        })
    }
}

fn should_rebuild_input(
    initialized: bool,
    inventory_changed: bool,
    force_monitor: bool,
    has_system_input: bool,
) -> bool {
    !initialized || inventory_changed || (force_monitor && has_system_input)
}

pub async fn recover_runtime() -> Result<()> {
    let systemd = UserSystemd::connect().await?;
    if systemd.recover_orphaned_game().await? {
        println!("Stopped orphaned zaman-game.service during recovery; returning to library.");
    }
    Ok(())
}

pub async fn monitor_menu_client(
    connection: Connection,
    commands: mpsc::Sender<ApiCommand>,
) -> Result<()> {
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?;
    let mut changes = proxy
        .receive_signal_with_args("NameOwnerChanged", &[(0, MENU_SERVICE)])
        .await?;

    use futures_util::StreamExt;
    while let Some(change) = changes.next().await {
        let (name, old_owner, new_owner): (String, String, String) = change.body().deserialize()?;
        if name == MENU_SERVICE && !old_owner.is_empty() && new_owner.is_empty() {
            if commands.send(ApiCommand::MenuClientExited).await.is_err() {
                return Ok(());
            }
        }
    }
    Err(message("D-Bus NameOwnerChanged stream closed"))
}

fn current_identity(menu: &MenuController) -> OperationIdentity {
    OperationIdentity {
        menu_generation: menu.snapshot().generation,
    }
}

fn begin_or_reject(
    slot: &mut OperationSlot,
    kind: OperationKind,
    menu: &MenuController,
    budget: Duration,
) -> std::result::Result<(crate::operations::OperationToken, OperationIdentity), String> {
    let identity = current_identity(menu);
    match slot.begin(kind, identity, budget, Instant::now().into()) {
        Ok(token) => Ok((token, identity)),
        Err(StartRejected::Busy { current }) => {
            Err(format!("busy: {} in flight", current.as_str()))
        }
        Err(StartRejected::ShuttingDown) => Err("shutting down".to_string()),
    }
}

pub async fn run_worker(
    mut commands: mpsc::Receiver<ApiCommand>,
    status: SharedStatus,
    menu: MenuController,
) -> Result<()> {
    let (mut system_input, mut input_monitor) = SystemInput::connect().await?;
    let mut sessions: JoinSet<Result<SessionOutcome>> = JoinSet::new();
    // Discovery only reads inventory. The worker remains the sole owner of
    // interception changes and monitor replacement. JoinSet aborts the pending
    // read when the worker exits.
    let mut discoveries: JoinSet<Result<Inventory>> = JoinSet::new();
    let mut active_control: Option<mpsc::Sender<SessionControl>> = None;
    let mut guide_buttons = BTreeMap::<String, GuideButton>::new();
    let mut input_listener_loss_reported = false;
    let mut input_refresh = interval_at(
        Instant::now() + INPUT_REFRESH_INTERVAL,
        INPUT_REFRESH_INTERVAL,
    );
    input_refresh.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut slot = OperationSlot::new();
    let mut slot_tick = interval_at(Instant::now() + SLOT_TICK, SLOT_TICK);
    slot_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(ApiCommand::Launch(launch)) => {
                        if active_control.is_some() {
                            status.fail("worker received a second launch while active");
                            continue;
                        }
                        let (token, identity) = match begin_or_reject(
                            &mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET,
                        ) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                status.fail(&format!("launch rejected: {reason}"));
                                continue;
                            }
                        };
                        // Startup already attempted discovery. Request a fresh
                        // inventory without holding up input or launching a
                        // second scan alongside the periodic one.
                        request_input_discovery(&mut discoveries, &system_input.input);
                        menu.game_started();
                        let (control, controls) = mpsc::channel(8);
                        active_control = Some(control);
                        sessions.spawn(run_session(launch, controls));
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::Stop) => {
                        let (token, identity) = match begin_or_reject(
                            &mut slot, OperationKind::Stop, &menu, STOP_BUDGET,
                        ) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                status.fail(&format!("stop rejected: {reason}"));
                                continue;
                            }
                        };
                        let _ = close_menu(&menu, &system_input, false, "session-stop").await;

                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::Stop).await;
                        }
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::Menu { action, reason, reply }) => {
                        let kind = match action {
                            MenuAction::Open => OperationKind::OpenMenu,
                            MenuAction::Close => OperationKind::CloseMenu,
                            MenuAction::Toggle if menu.snapshot().open => OperationKind::CloseMenu,
                            MenuAction::Toggle => OperationKind::OpenMenu,
                        };
                        let (token, identity) = match begin_or_reject(&mut slot, kind, &menu, MENU_BUDGET) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                let _ = reply.send(Err(reason));
                                continue;
                            }
                        };
                        let result = handle_menu_action(
                            action,
                            reason,
                            &menu,
                            &system_input,
                            active_control.is_some(),
                        )
                        .await
                        .map_err(|error| error.to_string());
                        let _ = reply.send(result);
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::MenuClientExited) => {
                        if menu.snapshot().open {
                            eprintln!("menu process exited unexpectedly");
                            eprintln!(
                                "restoring {}",
                                if active_control.is_some() { "game" } else { "library" }
                            );
                            if let Err(error) = system_input.set_mode(INTERCEPT_PASS).await {
                                eprintln!("Failed to restore controller input after menu exit: {error}");
                            }
                            menu.close("menu-process-exited", active_control.is_some());
                        }
                    }
                    Some(ApiCommand::ExitGame) => {
                        let (token, identity) = match begin_or_reject(
                            &mut slot, OperationKind::Stop, &menu, STOP_BUDGET,
                        ) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                status.fail(&format!("exit-game rejected: {reason}"));
                                continue;
                            }
                        };
                        let _ = close_menu(&menu, &system_input, false, "exit-game").await;
                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::ExitGame).await;
                            let _ = slot.complete(token, identity);
                        }
                    }
                    Some(ApiCommand::Reboot) => {
                        let (token, identity) = match begin_or_reject(
                            &mut slot, OperationKind::Reboot, &menu, SHUTDOWN_BUDGET,
                        ) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                status.fail(&format!("reboot rejected: {reason}"));
                                continue;
                            }
                        };
                        let _ = close_menu(&menu, &system_input, false, "reboot").await;
                        match &active_control {
                            Some(control) => {
                                status.mark_stopping();
                                if let Err(error) = control.send(SessionControl::Reboot).await {
                                    let error = format!("reboot control send failed: {error}");
                                    eprintln!("{error}");
                                    status.fail(error);
                                }
                            }
                            None => request_system_power(&status, "Reboot").await,
                        }
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::Shutdown) => {
                        let (token, identity) = match begin_or_reject(
                            &mut slot, OperationKind::PowerOff, &menu, SHUTDOWN_BUDGET,
                        ) {
                            Ok(pair) => pair,
                            Err(reason) => {
                                status.fail(&format!("shutdown rejected: {reason}"));
                                continue;
                            }
                        };
                        let _ = close_menu(&menu, &system_input, false, "shutdown").await;
                        match &active_control {
                            Some(control) => {
                                status.mark_stopping();
                                let _ = control.send(SessionControl::Shutdown).await;
                            }
                            None => request_system_power(&status, "PowerOff").await,
                        }
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::Quit) | None => {
                        if let Some(kind) = slot.abort_for_quit() {
                            println!(
                                "Quit preempted in-flight operation {}.",
                                kind.as_str()
                            );
                        }
                        let _ = close_menu(&menu, &system_input, false, "daemon-stop").await;
                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::Stop).await;
                        }
                        while let Some(result) = sessions.join_next().await {
                            record_completion(&status, result);
                        }
                        let _ = system_input.set_mode(INTERCEPT_NONE).await;
                        return Ok(());
                    }
                }
            }
            input_event = next_system_input(&mut input_monitor) => {
                match input_event {
                    Some(input_event) => {
                        input_listener_loss_reported = false;
                        handle_system_input(
                            input_event,
                            &menu,
                            &system_input,
                            active_control.is_some(),
                            &mut guide_buttons,
                        ).await?;
                    }
                    None => {
                        if !input_listener_loss_reported {
                            eprintln!("All InputPlumber event listeners stopped; reconnecting.");
                            input_listener_loss_reported = true;
                        }
                        input_monitor = None;
                        guide_buttons.clear();
                    }
                }
            }
            _ = input_refresh.tick() => {
                request_input_discovery(&mut discoveries, &system_input.input);
            }
            discovered = discoveries.join_next(), if !discoveries.is_empty() => {
                if let Some(result) = discovered {
                    let inventory = match result {
                        Ok(inventory) => inventory,
                        Err(error) => Err(message(format!(
                            "InputPlumber discovery task failed: {error}"
                        ))),
                    };
                    // Resolve mode and listener health now, not when the scan
                    // started: a menu transition may have occurred meanwhile.
                    let mode = if menu.snapshot().open { INTERCEPT_ALL } else { INTERCEPT_PASS };
                    let refresh = system_input.finish_discovery(
                        inventory,
                        mode,
                        input_monitor.is_none(),
                    ).await;
                    if refresh.replace_monitor {
                        input_monitor = refresh.monitor;
                        guide_buttons.clear();
                    }
                }
            }
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(result) = completed {
                    let power_method = requested_power_method(&result);
                    record_completion(&status, result);
                    active_control = None;
                    if menu.game_ended("game-ended") {
                        if let Err(error) = system_input.set_mode(INTERCEPT_PASS).await {
                            eprintln!("Failed to restore library input after game exit: {error}");
                        }
                    }
                    if let Some(method) = power_method {
                        request_system_power(&status, method).await;
                    }
                }
            }
            _ = slot_tick.tick() => {
                if let Some(kind) = slot.check_deadline(Instant::now().into()) {
                    eprintln!(
                        "Operation {} exceeded its budget; slot cleared.",
                        kind.as_str()
                    );
                }
            }
        }
    }
}

async fn next_system_input(monitor: &mut Option<SystemInputMonitor>) -> Option<SystemInputEvent> {
    match monitor.as_mut() {
        Some(monitor) => monitor.next().await,
        None => pending::<Option<SystemInputEvent>>().await,
    }
}

fn request_input_discovery(discoveries: &mut JoinSet<Result<Inventory>>, input: &InputPlumber) {
    // Includes completed-but-unconsumed results, so they cannot be overtaken
    // by a newer discovery. Missed requests are coalesced, not queued.
    if !discoveries.is_empty() {
        return;
    }
    let input = input.clone();
    discoveries.spawn(async move { input.discover().await });
}

async fn handle_system_input(
    event: SystemInputEvent,
    menu: &MenuController,
    input: &SystemInput,
    game_active: bool,
    guide_buttons: &mut BTreeMap<String, GuideButton>,
) -> Result<()> {
    match event {
        SystemInputEvent::Guide { target, action } => {
            let menu_open = menu.snapshot().open;
            let effect = guide_buttons
                .entry(target.clone())
                .or_default()
                .input(action, menu_open);
            match effect {
                GuideEffect::OpenMenu => {
                    println!("Guide press requested OpenMenu from {target}.");
                    if let Err(error) =
                        handle_menu_action(MenuAction::Open, "guide", menu, input, game_active)
                            .await
                    {
                        eprintln!("Guide menu request failed: {error}");
                    }
                }
                GuideEffect::CloseOnReleaseArmed => {
                    println!("Guide press armed CloseMenu from {target}; waiting for release.");
                }
                GuideEffect::CloseMenu => {
                    println!("Guide release requested CloseMenu from {target}.");
                    if let Err(error) =
                        handle_menu_action(MenuAction::Close, "guide", menu, input, game_active)
                            .await
                    {
                        eprintln!("Guide menu request failed: {error}");
                    }
                }
                GuideEffect::None if action == GuideAction::Released => {
                    println!("Guide released by {target}.");
                }
                GuideEffect::None => {}
            }
        }
        SystemInputEvent::MenuInput { event, value } => {
            let forwarded = menu.input(event.clone(), value);
            if forwarded && value > 0.5 {
                println!("System menu input event={event} value={value}.");
            }
        }
    }
    Ok(())
}

async fn handle_menu_action(
    action: MenuAction,
    reason: &'static str,
    menu: &MenuController,
    input: &SystemInput,
    game_active: bool,
) -> Result<()> {
    match action {
        MenuAction::Open => open_menu(menu, input, reason, game_active).await,
        MenuAction::Close => close_menu(menu, input, game_active, reason).await,
        MenuAction::Toggle if menu.snapshot().open => {
            close_menu(menu, input, game_active, reason).await
        }
        MenuAction::Toggle => open_menu(menu, input, reason, game_active).await,
    }
}

async fn open_menu(
    menu: &MenuController,
    input: &SystemInput,
    reason: &'static str,
    game_active: bool,
) -> Result<()> {
    if menu.snapshot().open {
        println!("OpenMenu ignored because the menu is already active.");
        return Ok(());
    }

    ensure_menu_client().await?;
    if let Err(error) = input.set_mode(INTERCEPT_ALL).await {
        if let Err(restore_error) = input.set_mode(INTERCEPT_PASS).await {
            eprintln!(
                "Failed to restore controller input after menu interception failed: {restore_error}"
            );
        }
        return Err(error);
    }
    let Some(generation) = menu.open(reason) else {
        input.set_mode(INTERCEPT_PASS).await?;
        return Ok(());
    };

    if menu
        .wait_until_presented(generation, MENU_PRESENT_TIMEOUT)
        .await
    {
        println!("menu generation={generation} presented");
        return Ok(());
    }

    eprintln!("menu generation={generation} failed to present; restoring previous foreground");
    menu.close("menu-launch-failed", game_active);
    let restore_result = input.set_mode(INTERCEPT_PASS).await;
    restore_result?;
    Err(message(
        "zaman-menu did not present its surface within 3 seconds",
    ))
}

async fn close_menu(
    menu: &MenuController,
    input: &SystemInput,
    game_active: bool,
    reason: &'static str,
) -> Result<()> {
    if !menu.snapshot().open {
        println!("CloseMenu ignored because the menu is already closed.");
        return Ok(());
    }

    let input_result = input.set_mode(INTERCEPT_PASS).await;
    menu.close(reason, game_active);
    input_result
}

async fn ensure_menu_client() -> Result<()> {
    let connection = Connection::session().await?;
    if name_has_owner(&connection).await? {
        return Ok(());
    }

    println!("zaman-menu is unavailable; requesting zaman-menu.service start.");
    UserSystemd::connect().await?.start_menu().await?;
    for _ in 0..60 {
        if name_has_owner(&connection).await? {
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }
    Err(message("zaman-menu D-Bus client did not become ready"))
}

async fn name_has_owner(connection: &Connection) -> Result<bool> {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?;
    Ok(proxy.call("NameHasOwner", &(MENU_SERVICE)).await?)
}

async fn run_session(
    launch: ResolvedLaunch,
    controls: mpsc::Receiver<SessionControl>,
) -> Result<SessionOutcome> {
    println!(
        "Launching system={} emulator={} ({}) ROM={}",
        launch.system_id, launch.emulator_id, launch.emulator_name, launch.rom_path
    );
    let systemd = UserSystemd::connect().await?;
    let mut session = Session::new(systemd);
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

fn requested_power_method(
    completion: &std::result::Result<Result<SessionOutcome>, tokio::task::JoinError>,
) -> Option<&'static str> {
    match completion {
        Ok(Ok(SessionOutcome::RebootRequested)) => Some("Reboot"),
        Ok(Ok(SessionOutcome::ShutdownRequested)) => Some("PowerOff"),
        _ => None,
    }
}

async fn request_system_power(status: &SharedStatus, method: &'static str) {
    async fn inner(method: &'static str) -> zbus::Result<()> {
        let connection = zbus::Connection::system().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        proxy.call::<_, _, ()>(method, &(true)).await
    }

    if let Err(error) = inner(method).await {
        let error = format!("System {method} failed: {error}");
        eprintln!("{error}");
        status.fail(error);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        begin_or_reject, current_identity, should_rebuild_input, LAUNCH_BUDGET, MENU_BUDGET,
    };
    use crate::menu::MenuController;
    use crate::operations::{CompleteRejected, OperationIdentity, OperationKind, OperationSlot};

    #[test]
    fn initial_discovery_is_always_recorded() {
        assert!(should_rebuild_input(false, false, false, false));
    }

    #[test]
    fn hotplug_inventory_change_rebuilds_the_monitor() {
        assert!(should_rebuild_input(true, true, false, true));
        assert!(should_rebuild_input(true, true, false, false));
    }

    #[test]
    fn listener_loss_reconnects_an_unchanged_available_target() {
        assert!(should_rebuild_input(true, false, true, true));
    }

    #[test]
    fn stable_or_still_empty_inventory_does_not_churn() {
        assert!(!should_rebuild_input(true, false, false, true));
        assert!(!should_rebuild_input(true, false, true, false));
    }

    #[test]
    fn begin_or_reject_accepts_empty_slot() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        let result = begin_or_reject(&mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET);
        assert!(result.is_ok());
        assert!(slot.is_busy());
    }

    #[test]
    fn begin_or_reject_reports_busy_with_current_kind() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        let _ = begin_or_reject(&mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET);
        let err = begin_or_reject(&mut slot, OperationKind::Stop, &menu, LAUNCH_BUDGET)
            .expect_err("expected Busy");
        assert!(err.contains("busy"));
        assert!(err.contains("launch"));
    }

    #[test]
    fn begin_or_reject_reports_shutting_down() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        slot.begin_shutdown();
        let err = begin_or_reject(&mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET)
            .expect_err("expected ShuttingDown");
        assert!(err.contains("shutting down"));
    }

    #[test]
    fn current_identity_tracks_menu_generation() {
        let (menu, _rx) = MenuController::new();
        assert_eq!(current_identity(&menu).menu_generation, 0);
        let _ = menu.open("test");
        assert_eq!(current_identity(&menu).menu_generation, 1);
    }

    #[test]
    fn completion_with_captured_identity_succeeds() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        let (token, identity) =
            begin_or_reject(&mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET).unwrap();
        // The operation itself bumps the menu generation. The captured
        // identity, not the current one, is what completion validates.
        let _ = menu.open("during-launch");
        let kind = slot.complete(token, identity).expect("complete");
        assert_eq!(kind, OperationKind::Launch);
        assert!(!slot.is_busy());
    }

    #[test]
    fn completion_with_stale_identity_is_rejected() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        let (token, _identity) =
            begin_or_reject(&mut slot, OperationKind::Launch, &menu, LAUNCH_BUDGET).unwrap();
        // Simulate a caller presenting an identity from a different epoch.
        let stale = OperationIdentity {
            menu_generation: 999,
        };
        let err = slot.complete(token, stale).expect_err("expected rejection");
        assert_eq!(err, CompleteRejected::IdentityChanged);
        assert!(slot.is_busy());
    }

    #[test]
    fn abort_for_quit_clears_slot() {
        let (menu, _rx) = MenuController::new();
        let mut slot = OperationSlot::new();
        let _ = begin_or_reject(&mut slot, OperationKind::Launch, &menu, MENU_BUDGET);
        assert_eq!(slot.abort_for_quit(), Some(OperationKind::Launch));
        assert!(!slot.is_busy());
    }

    #[tokio::test]
    async fn power_dispatch_requires_successful_session_completion() {
        use crate::error::message;
        use crate::session::SessionOutcome;
        assert_eq!(
            super::requested_power_method(&Ok(Ok(SessionOutcome::RebootRequested))),
            Some("Reboot")
        );
        assert_eq!(
            super::requested_power_method(&Ok(Ok(SessionOutcome::ShutdownRequested))),
            Some("PowerOff")
        );
        for outcome in [
            SessionOutcome::GameExited,
            SessionOutcome::StopRequested,
            SessionOutcome::ExitGameRequested,
        ] {
            assert_eq!(super::requested_power_method(&Ok(Ok(outcome))), None);
        }
        assert_eq!(
            super::requested_power_method(&Ok(Err(message("stop failed")))),
            None
        );
        let task = tokio::spawn(std::future::pending::<crate::error::Result<SessionOutcome>>());
        task.abort();
        assert_eq!(super::requested_power_method(&task.await), None);
    }
}
