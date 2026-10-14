use crate::api::{ApiCommand, MenuAction, SharedStatus};
use crate::error::{message, Result};
use crate::guide::{GuideAction, GuideRouter, RoutedGuide};
use crate::inputplumber::{
    InputPlumber, Inventory, InventoryRefreshQueue, SystemInputEvent, SystemInputMonitor, INTERCEPT_ALL, INTERCEPT_NONE,
    INTERCEPT_PASS,
};
use crate::menu::MenuController;
use crate::operations::{OperationIdentity, OperationKind, OperationSlot, StartRejected};
use crate::registry::ResolvedLaunch;
use crate::session::{Session, SessionControl, SessionOutcome};
use crate::systemd::UserSystemd;
use std::collections::BTreeSet;
use std::future::{pending, Future};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::{interval_at, sleep, Instant, MissedTickBehavior};

const INPUT_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const INPUT_RECOVERY_INTERVAL: Duration = Duration::from_secs(1);
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

#[derive(Default)]
struct InputMode {
    dirty: AtomicBool,
}

impl InputMode {
    fn dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    async fn apply(&self, update: impl Future<Output = Result<()>>) -> Result<()> {
        // Mark before awaiting so cancellation also leaves reconciliation due.
        self.dirty.store(true, Ordering::Relaxed);
        let result = update.await;
        if result.is_ok() {
            self.dirty.store(false, Ordering::Relaxed);
        }
        result
    }
}

struct SystemInput {
    input: InputPlumber,
    inventory: Inventory,
    initialized: bool,
    refresh_error: Option<String>,
    mode: InputMode,
}

impl SystemInput {
    async fn connect() -> Result<(Self, Option<SystemInputMonitor>)> {
        let input = InputPlumber::connect().await?;
        let mut system_input = Self {
            input,
            inventory: Inventory::default(),
            initialized: false,
            refresh_error: None,
            mode: InputMode::default(),
        };
        let refresh = system_input.refresh(INTERCEPT_PASS, true).await;
        Ok((system_input, refresh.monitor))
    }

    async fn set_mode(&self, mode: u32) -> Result<()> {
        self.mode.apply(self.input.set_intercept_mode(&self.inventory, mode)).await
    }

    async fn refresh(&mut self, mode: u32, force_monitor: bool) -> InputRefresh {
        let inventory = self.input.discover().await;
        self.finish_discovery(inventory, Some(mode), force_monitor).await
    }

    async fn finish_discovery(
        &mut self,
        inventory: Result<Inventory>,
        mode: Option<u32>,
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
        mode: Option<u32>,
        force_monitor: bool,
    ) -> Result<InputRefresh> {
        let changed = !self.initialized || inventory != self.inventory;

        if !should_reconcile_input(
            self.initialized,
            changed,
            force_monitor,
            inventory.has_system_input(),
            self.mode.dirty(),
        ) {
            return Ok(InputRefresh::unchanged());
        }

        if inventory.has_system_input() {
            if let Some(mode) = mode {
                self.mode.apply(self.input.set_intercept_mode(&inventory, mode)).await?;
            }
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

fn should_reconcile_input(
    initialized: bool,
    inventory_changed: bool,
    force_monitor: bool,
    has_system_input: bool,
    mode_dirty: bool,
) -> bool {
    should_rebuild_input(initialized, inventory_changed, force_monitor, has_system_input)
        || (mode_dirty && has_system_input)
}

// Reconciliation failure must never bypass the already-established game and
// input shutdown paths. Report errors only after every cleanup was attempted.
async fn finish_shutdown(
    transfer: impl Future<Output = Result<()>>,
    session: impl Future<Output = Result<()>>,
    input: impl Future<Output = Result<()>>,
) -> Result<()> {
    let transfer = transfer.await;
    if let Err(error) = &transfer {
        eprintln!("Transfer shutdown cleanup failed: {error}");
    }
    let session = session.await;
    let input = input.await;
    transfer.and(session).and(input)
}

pub async fn recover_runtime() -> Result<()> {
    let systemd = UserSystemd::connect().await?;
    systemd.stop_transfer().await?;
    for id in ["dukkan", "steam"] { systemd.stop_storefront(id).await?; }
    if systemd.recover_orphaned_game().await? {
        println!("Stopped orphaned zaman-game.service during recovery; returning to library.");
    }
    if let Err(error) = crate::sway::SwayController::from_environment().game_ended().await {
        eprintln!("Sway startup library restoration: {error}");
    }
    Ok(())
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
    let mut inventory_monitor = system_input.input.monitor_inventory();
    let mut inventory_refresh = InventoryRefreshQueue::default();
    let mut input_recovery = interval_at(
        Instant::now() + INPUT_RECOVERY_INTERVAL,
        INPUT_RECOVERY_INTERVAL,
    );
    input_recovery.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sessions: JoinSet<Result<SessionOutcome>> = JoinSet::new();
    let mut storefront_tasks = JoinSet::new();
    let mut storefront_controls = std::collections::BTreeMap::<String, mpsc::Sender<()>>::new();
    let mut transfers = JoinSet::new();
    let mut transfer_stop: Option<watch::Sender<bool>> = None;
    let mut transfer_pressed = BTreeSet::<String>::new();
    let mut input_epoch = menu.transfer_context();
    // Discovery only reads inventory. The worker remains the sole owner of
    // interception changes and monitor replacement. JoinSet aborts the pending
    // read when the worker exits.
    let mut discoveries: JoinSet<Result<Inventory>> = JoinSet::new();
    let mut active_control: Option<mpsc::Sender<SessionControl>> = None;
    let mut guide_buttons = GuideRouter::default();
    let mut input_listener_loss_reported = false;
    let mut input_refresh = interval_at(
        Instant::now() + INPUT_REFRESH_INTERVAL,
        INPUT_REFRESH_INTERVAL,
    );
    input_refresh.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut slot = OperationSlot::new();
    let mut slot_tick = interval_at(Instant::now() + SLOT_TICK, SLOT_TICK);
    slot_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut compositor_tick = interval_at(Instant::now() + Duration::from_millis(250),
        Duration::from_millis(250));
    compositor_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut compositor_error: Option<String> = None;
    let mut delegated_guide = false;

    loop {
        tokio::select! {
            command = commands.recv() => {
                let command = match command {
                    Some(ApiCommand::Guarded { command, reply }) => {
                        if menu.transfer_busy() {
                            let _ = reply.send(Err("busy: close Transfer Games first".into()));
                            continue;
                        }
                        let _ = reply.send(Ok(()));
                        Some(*command)
                    }
                    command => command,
                };
                // Recheck in the worker: commands may have queued before the
                // transfer reservation became visible to D-Bus callers.
                if menu.transfer_busy() && matches!(&command,
                    Some(ApiCommand::Launch(_)) | Some(ApiCommand::Stop) |
                    Some(ApiCommand::ExitGame) | Some(ApiCommand::Reboot) | Some(ApiCommand::Shutdown)) {
                    status.fail("busy: close Transfer Games first");
                    continue;
                }
                match command {
                    Some(ApiCommand::Guarded { .. }) => unreachable!("nested guarded command"),
                    Some(ApiCommand::Storefront { action, id, generation, reply }) => {
                        let result = async {
                            if !menu.compositor.enabled() { return Err(message("managed storefronts require the graphical Sway environment")); }
                            if slot.is_busy() || active_control.is_some()
                                || matches!(status.snapshot().state.as_str(), "Active" | "Stopping") {
                                return Err(message("busy: a game or session operation is in flight"));
                            }
                            // QML must hide its layer and release input before changing focus.
                            if crate::quickshell_menu::status().await?.input_owned {
                                return Err(message("close the Guide menu before changing storefronts"));
                            }
                            crate::storefront::unit(&id)?;
                            match action.as_str() {
                                "launch" => {
                                    if menu.storefronts.has_game() && menu.storefronts.active().as_deref() != Some(id.as_str()) {
                                        return Err(message("close the storefront game before switching"));
                                    }
                                    if menu.storefronts.entry(&id).is_some_and(|e| e.phase == "Running") {
                                        menu.storefronts.activate(&id)?;
                                    } else {
                                        let config = crate::storefront::Config::load(&id)?;
                                        let command = config.command()?;
                                        let generation = menu.storefronts.reserve(&id, config.native_guide)?;
                                        let (control, controls) = mpsc::channel(1);
                                        storefront_controls.insert(id.clone(), control);
                                        storefront_tasks.spawn(crate::storefront::run(id.clone(), generation, command,
                                            menu.storefronts.clone(), menu.compositor.clone(), controls));
                                    }
                                    menu.storefront_foreground(true);
                                    menu.compositor.activate_storefront(Some(id.clone())).await?;
                                }
                                "home" | "close" => {
                                    menu.storefronts.validate_leave(&id, generation, action == "close")?;
                                    if action == "close" {
                                        menu.storefronts.update(&id, generation, "Stopping", String::new());
                                        storefront_controls.get(&id).ok_or_else(|| message("storefront supervisor unavailable"))?
                                            .send(()).await.map_err(|_| message("storefront supervisor stopped"))?;
                                    } else {
                                        menu.storefronts.home();
                                        menu.storefront_foreground(false);
                                        menu.compositor.activate_storefront(None).await?;
                                    }
                                }
                                _ => return Err(message("unknown storefront action")),
                            }
                            system_input.set_mode(if menu.storefronts.native_guide() { INTERCEPT_NONE } else { INTERCEPT_PASS }).await
                        }.await.map_err(|e| e.to_string());
                        guide_buttons.clear();
                        let _ = reply.send(result);
                    }
                    Some(ApiCommand::StartTransfer { menu_generation, reply }) => {
                        let reserved = if slot.is_busy() || active_control.is_some()
                            || matches!(status.snapshot().state.as_str(), "Active" | "Stopping") {
                            Err("busy: a game or session operation is in flight".into())
                        } else { menu.reserve_transfer(menu_generation, false) };
                        match reserved {
                            Err(reason) => { let _ = reply.send(Err(reason)); }
                            Ok(generation) => {
                                guide_buttons.clear();
                                transfer_pressed.clear();
                                let (stop, stopped) = watch::channel(false);
                                transfer_stop = Some(stop);
                                transfers.spawn(crate::transfer::run(
                                    menu.clone(), generation, stopped,
                                    system_input.input.clone(), system_input.inventory.clone(),
                                ));
                                let _ = reply.send(Ok(generation));
                            }
                        }
                    }
                    Some(ApiCommand::Launch(launch)) => {
                        if active_control.is_some() || menu.storefronts.active().is_some() || menu.storefronts.has_game() {
                            status.fail("worker received a launch while a game or storefront owns the foreground");
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
                        sessions.spawn(run_session(launch, controls, menu.compositor.clone()));
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

                        if let Some(control) = &active_control {
                            status.mark_stopping();
                            let _ = control.send(SessionControl::Stop).await;
                        }
                        let _ = slot.complete(token, identity);
                    }
                    Some(ApiCommand::Menu { action, reason, reply }) => {
                        if menu.transfer_busy() {
                            let _ = reply.send(Err("busy: close Transfer Games first".into()));
                            continue;
                        }
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
                        if menu.transfer_busy() {
                            menu.transfer_menu_lost();
                            if let Some(stop) = &transfer_stop { stop.send_replace(true); }
                            continue;
                        }
                        if menu.snapshot().open {
                            eprintln!("menu process exited unexpectedly");
                            eprintln!(
                                "restoring {}",
                                if active_control.is_some() { "game" } else { "library" }
                            );
                            if let Err(error) = close_menu(&menu, &system_input,
                                active_control.is_some(), "menu-process-exited").await {
                                eprintln!("Failed to restore foreground after menu exit: {error}");
                            }
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
                        if let Some(stop) = transfer_stop.take() { stop.send_replace(true); }
                        while let Some(completion) = transfers.join_next().await {
                            if let Ok(completion) = completion {
                                if completion.cleaned { menu.finish_transfer(completion.generation, completion.error); }
                            }
                        }
                        // A failed fresh connection must not bypass menu,
                        // game, or input cleanup (including non-transfer sessions).
                        return finish_shutdown(
                            async { UserSystemd::connect().await?.stop_transfer().await },
                            async {
                                if let Some(kind) = slot.abort_for_quit() {
                                    println!("Quit preempted in-flight operation {}.", kind.as_str());
                                }
                                let _ = close_menu(&menu, &system_input, false, "daemon-stop").await;
                                if let Some(control) = &active_control {
                                    status.mark_stopping();
                                    let _ = control.send(SessionControl::Stop).await;
                                }
                                while let Some(result) = sessions.join_next().await {
                                    record_completion(&status, result);
                                }
                                for control in storefront_controls.values() { let _ = control.send(()).await; }
                                while let Some(result) = storefront_tasks.join_next().await {
                                    if let Ok((id, _, result)) = result {
                                        if let Err(error) = result { eprintln!("Storefront shutdown: {error}"); }
                                        let _ = menu.compositor.storefront_ended(id).await;
                                    }
                                }
                                menu.compositor.game_ended().await
                            },
                            system_input.set_mode(INTERCEPT_NONE),
                        ).await;
                    }
                }
            }
            input_event = next_system_input(&mut input_monitor) => {
                match input_event {
                    Some(input_event) => {
                        input_listener_loss_reported = false;
                        let epoch = menu.transfer_context();
                        if epoch != input_epoch {
                            guide_buttons.clear();
                            transfer_pressed.clear();
                            input_epoch = epoch;
                        }
                        if menu.transfer_busy() {
                            let (event, value) = match input_event {
                                SystemInputEvent::MenuInput { event, value } => (if event == "ui_cancel" { "ui_back".into() } else { event }, value),
                                SystemInputEvent::Guide { action, .. } => ("ui_guide".into(), if action == GuideAction::Released { 0.0 } else { 1.0 }),
                            };
                            if menu.transfer_context().1 == "active" {
                                let forward = if value > 0.5 { transfer_pressed.insert(event.clone()) } else { transfer_pressed.remove(&event) };
                                if forward { menu.transfer_input(event, value); }
                            }
                            continue;
                        }
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
                        transfer_pressed.clear();
                        inventory_refresh.request(Instant::now());
                    }
                }
            }
            change = inventory_monitor.next() => {
                if change.is_some() {
                    guide_buttons.clear();
                    inventory_refresh.request(Instant::now());
                } else {
                    inventory_monitor = system_input.input.monitor_inventory();
                    inventory_refresh.request(Instant::now());
                }
            }
            _ = wait_inventory_refresh(inventory_refresh.deadline()), if discoveries.is_empty() => {
                inventory_refresh.started();
                request_input_discovery(&mut discoveries, &system_input.input);
            }
            _ = input_recovery.tick() => {
                if should_retry_input(
                    system_input.initialized,
                    system_input.refresh_error.is_some(),
                    system_input.mode.dirty(),
                    system_input.inventory.has_system_input(),
                    input_monitor.is_some(),
                ) {
                    request_input_discovery(&mut discoveries, &system_input.input);
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
                    let shell_open = crate::quickshell_menu::status().await
                        .map(|s| s.input_owned).unwrap_or(true);
                    // Resident acquisition and hotplug recovery own interception.
                    // A daemon ALL write would strand a newly connected controller
                    // because the helper correctly refuses an unknown ALL owner.
                    let mode = if menu.snapshot().open || menu.transfer_busy() { Some(INTERCEPT_ALL) }
                        else if shell_open { None }
                        else if menu.storefronts.native_guide() { Some(INTERCEPT_NONE) } else { Some(INTERCEPT_PASS) };
                    let refresh = system_input.finish_discovery(
                        inventory,
                        mode,
                        input_monitor.is_none(),
                    ).await;
                    if refresh.replace_monitor {
                        input_monitor = refresh.monitor;
                        guide_buttons.clear();
                        transfer_pressed.clear();
                    }
                }
            }
            completed = transfers.join_next(), if !transfers.is_empty() => {
                transfer_stop = None;
                guide_buttons.clear();
                transfer_pressed.clear();
                match completed {
                    Some(Ok(completion)) if completion.cleaned => {
                        if let Some(error) = &completion.error { eprintln!("{error}"); }
                        if menu.finish_transfer(completion.generation, completion.error) {
                            let mode = if menu.snapshot().open { INTERCEPT_ALL } else { INTERCEPT_PASS };
                            if let Err(error) = system_input.set_mode(mode).await {
                                eprintln!("Transfer input restoration failed: {error}");
                            }
                        }
                    }
                    Some(Ok(completion)) => {
                        let error = completion.error.unwrap_or_else(|| "transfer cleanup failed".into());
                        eprintln!("{error}");
                        menu.transfer_cleanup_failed(completion.generation, error.clone());
                        status.fail(error);
                    }
                    Some(Err(error)) => {
                        eprintln!("Transfer supervisor failed: {error}; cleaning attempted service.");
                        let generation = menu.transfer_context().0;
                        let (stop, stopped) = watch::channel(true);
                        transfer_stop = Some(stop);
                        transfers.spawn(crate::transfer::run(
                            menu.clone(), generation, stopped,
                            system_input.input.clone(), system_input.inventory.clone(),
                        ));
                    }
                    None => {}
                }
            }
            completed = storefront_tasks.join_next(), if !storefront_tasks.is_empty() => {
                if let Some(result) = completed {
                    match result {
                        Ok((id, generation, result)) => {
                            let was_active = menu.storefronts.active().as_deref() == Some(id.as_str());
                            let error = result.err().map(|e| e.to_string()).unwrap_or_default();
                            menu.storefronts.update(&id, generation, if error.is_empty() { "Exited" } else { "Failed" }, error.clone());
                            storefront_controls.remove(&id);
                            if let Err(error) = menu.compositor.storefront_ended(id.clone()).await {
                                eprintln!("Storefront focus recovery: {error}");
                            }
                            if was_active {
                                menu.storefront_foreground(false);
                                guide_buttons.clear();
                                if let Err(error) = system_input.set_mode(INTERCEPT_PASS).await { eprintln!("Storefront input recovery: {error}"); }
                            }
                            if !error.is_empty() { eprintln!("Storefront {id} failed: {error}"); }
                        }
                        Err(error) => {
                            // A panicked supervisor has uncertain cleanup. Do not silently
                            // permit a replacement to inherit its unit or input ownership.
                            eprintln!("Storefront supervisor failed; restart sessiond for recovery: {error}");
                        }
                    }
                }
            }
            completed = sessions.join_next(), if !sessions.is_empty() => {
                if let Some(result) = completed {
                    let power_method = requested_power_method(&result);
                    record_completion(&status, result);
                    active_control = None;
                    let menu_was_open = menu.game_ended("game-ended");
                    if let Err(error) = menu.compositor.game_ended().await {
                        eprintln!("Sway library restoration after game exit: {error}");
                    }
                    if menu_was_open {
                        if let Err(error) = system_input.set_mode(INTERCEPT_PASS).await {
                            eprintln!("Failed to restore library input after game exit: {error}");
                        }
                    }
                    if let Some(method) = power_method {
                        request_system_power(&status, method).await;
                    }
                }
            }
            _ = compositor_tick.tick() => {
                // Transfer retains foreground ownership until its existing
                // supervisor confirms cleanup; never steal focus from it.
                if !menu.transfer_busy() {
                    let native = menu.storefronts.native_guide();
                    if native != delegated_guide {
                        if crate::quickshell_menu::status().await.map(|s| !s.input_owned).unwrap_or(false) {
                            if system_input.set_mode(if native { INTERCEPT_NONE } else { INTERCEPT_PASS }).await.is_ok() {
                                delegated_guide = native;
                                guide_buttons.clear();
                            }
                        }
                    }
                    match menu.compositor.steam_game_running().await {
                        Ok(game) => menu.storefronts.set_game("steam", game),
                        Err(error) => eprintln!("Steam game classification unavailable: {error}"),
                    }
                    match menu.compositor.reconcile(menu.snapshot().open, active_control.is_some()).await {
                        Ok(()) => { if compositor_error.take().is_some() { println!("Sway supervision recovered."); } }
                        Err(error) => {
                            let error = error.to_string();
                            if compositor_error.as_deref() != Some(error.as_str()) {
                                eprintln!("Sway supervision: {error}; retrying.");
                                compositor_error = Some(error);
                            }
                        }
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

async fn wait_inventory_refresh(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending::<()>().await,
    }
}

fn should_retry_input(
    initialized: bool,
    refresh_failed: bool,
    mode_dirty: bool,
    has_system_input: bool,
    listener_present: bool,
) -> bool {
    !initialized || refresh_failed || (has_system_input && (mode_dirty || !listener_present))
}

fn request_input_discovery(discoveries: &mut JoinSet<Result<Inventory>>, input: &InputPlumber) {
    // Includes completed-but-unconsumed results, so they cannot be overtaken
    // by a newer discovery. InventoryRefreshQueue separately retains hotplug
    // invalidations that arrive during a scan; periodic requests can be dropped.
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
    _game_active: bool,
    guide_buttons: &mut GuideRouter,
) -> Result<()> {
    if menu.storefronts.native_guide() {
        if let SystemInputEvent::Guide { .. } = &event {
            if crate::quickshell_menu::status().await.map(|s| !s.presented_or_pending()).unwrap_or(false) {
                guide_buttons.clear();
                return Ok(());
            }
        }
    }
    match event {
        SystemInputEvent::Guide { target, action } => {
            match action {
                GuideAction::Pressed if guide_buttons.needs_status(&target) => {
                    match crate::quickshell_menu::status().await {
                        Ok(status) => {
                            let identity = if status.status == "open" {
                                match tokio::time::timeout(Duration::from_millis(750), input.input.guide_identity(&target)).await {
                                    Ok(Ok(Some(identity))) => identity,
                                    _ => {
                                        guide_buttons.suppress_press(&target);
                                        crate::quickshell_menu::log("Guide close not armed: source identity unavailable");
                                        return Ok(());
                                    }
                                }
                            } else { String::new() };
                            let effect = guide_buttons.press(&target, &status.status, &status.session_key, &identity);
                            if effect == RoutedGuide::Open {
                                // PASS automatically became ALL on this Guide press.
                                // Release only that source's automatic interception;
                                // the resident helper must acquire and restore PASS.
                                match tokio::time::timeout(Duration::from_millis(750), input.input.release_guide_activation(&target)).await {
                                    Ok(Ok(())) => {},
                                    result => {
                                        crate::quickshell_menu::log(&format!("Guide activation handoff failed: {result:?}"));
                                        return Ok(());
                                    }
                                }
                                if let Err(error) = crate::quickshell_menu::dispatch(MenuAction::Open).await {
                                    eprintln!("Guide Quickshell open failed: {error}");
                                }
                            }
                        }
                        Err(error) => {
                            guide_buttons.suppress_press(&target);
                            crate::quickshell_menu::log(&format!("Guide status failed: {error}"));
                            eprintln!("Guide Quickshell status failed: {error}");
                        },
                    }
                }
                GuideAction::Pressed => guide_buttons.suppress_press(&target),
                GuideAction::Released => {
                    if let Some(armed_session) = guide_buttons.release(&target) {
                        let same_source = matches!(
                            tokio::time::timeout(Duration::from_millis(750), input.input.guide_identity(&target)).await,
                            Ok(Ok(Some(identity))) if identity == armed_session.identity
                        );
                        if !same_source {
                            crate::quickshell_menu::log("Guide release ignored: controller disappeared or changed");
                            return Ok(());
                        }
                        if let Err(error) = crate::quickshell_menu::close_session(armed_session.session).await {
                            crate::quickshell_menu::log(&format!("Guide close failed: {error}"));
                            eprintln!("Guide Quickshell close failed: {error}");
                        }
                    }
                }
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
    _reason: &'static str,
    menu: &MenuController,
    _input: &SystemInput,
    _game_active: bool,
) -> Result<()> {
    if menu.transfer_busy() { return Err(message("busy: close Transfer Games first")); }
    // Quickshell's helper owns the overlay's focus/input lease and restoration.
    // Do not also invoke the retired SDL compositor/interception path here.
    crate::quickshell_menu::dispatch(action).await
}

async fn close_menu(
    menu: &MenuController,
    input: &SystemInput,
    game_active: bool,
    reason: &'static str,
) -> Result<()> {
    if !menu.snapshot().open {
        println!("CloseMenu ignored because the menu is already closed.");
        let compositor_result = menu.compositor.finish_menu(game_active).await;
        let input_result = input.set_mode(INTERCEPT_PASS).await;
        return compositor_result.and(input_result);
    }

    menu.close(reason, game_active);
    let compositor_result = menu.compositor.finish_menu(game_active).await;
    // Always attempt input restoration, including compositor transport failure.
    let input_result = input.set_mode(INTERCEPT_PASS).await;
    compositor_result.and(input_result)
}

async fn run_session(
    launch: ResolvedLaunch,
    controls: mpsc::Receiver<SessionControl>,
    compositor: crate::sway::SwayController,
) -> Result<SessionOutcome> {
    println!(
        "Launching system={} emulator={} ({}) ROM={}",
        launch.system_id, launch.emulator_id, launch.emulator_name, launch.rom_path
    );
    let systemd = UserSystemd::connect().await?;
    let mut session = Session::new(systemd, compositor);
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
    #[tokio::test]
    async fn transfer_manager_failure_does_not_skip_game_menu_or_input_shutdown() {
        let steps = std::cell::RefCell::new(Vec::new());
        let (menu, _) = MenuController::new();
        menu.game_started();
        menu.open("shutdown-regression");
        let (control, mut commands) = tokio::sync::mpsc::channel(1);
        let input_released = std::cell::Cell::new(false);
        let result = super::finish_shutdown(
            async { steps.borrow_mut().push("transfer-connect"); Err(crate::error::message("Subscribe failed")) },
            async {
                assert!(menu.close("daemon-stop", true));
                steps.borrow_mut().push("close-menu");
                control.send(crate::session::SessionControl::Stop).await.unwrap();
                steps.borrow_mut().extend(["stop-game", "join-game"]);
                Ok(())
            },
            async { input_released.set(true); steps.borrow_mut().push("release-input"); Ok(()) },
        ).await;
        assert!(result.is_err());
        assert!(!menu.snapshot().open);
        assert!(matches!(commands.try_recv(), Ok(crate::session::SessionControl::Stop)));
        assert!(input_released.get());
        assert_eq!(*steps.borrow(), vec!["transfer-connect", "close-menu", "stop-game", "join-game", "release-input"]);
    }

    #[tokio::test]
    async fn transfer_restoration_failure_retries_unchanged_healthy_inventory() {
        let mode = super::InputMode::default();
        assert!(!mode.dirty());
        assert!(mode.apply(async { Err(crate::error::message("temporary property write failure")) }).await.is_err());
        assert!(super::should_reconcile_input(true, false, false, true, mode.dirty()));
        mode.apply(async { Ok(()) }).await.unwrap();
        assert!(!super::should_reconcile_input(true, false, false, true, mode.dirty()));
    }

}


#[cfg(test)]
mod idle_input_regression {
    use super::should_retry_input;

    #[test]
    fn healthy_idle_and_zero_controllers_do_not_poll_each_second() {
        assert!(!should_retry_input(true, false, false, true, true));
        assert!(!should_retry_input(true, false, false, false, false));
        assert!(!should_retry_input(true, false, true, false, false));
    }

    #[test]
    fn failed_discovery_lost_listener_and_cancelled_mode_get_fast_recovery() {
        assert!(should_retry_input(false, false, false, false, false));
        assert!(should_retry_input(true, true, false, false, false));
        assert!(should_retry_input(true, false, false, true, false));
        assert!(should_retry_input(true, false, true, true, true));
    }
}
