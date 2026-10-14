use futures_util::StreamExt;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::Canvas;
use sdl2::rwops::RWops;
use sdl2::ttf::{Font, Sdl2TtfContext};
use sdl2::video::Window;
use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::{interval, interval_at, Instant, MissedTickBehavior};
use zaman_sessiond::contract::{MenuContextTuple, INTERFACE, MENU_SERVICE, PATH, SERVICE};
use zbus::{connection::Builder, Connection, Proxy};
#[path = "zaman-menu/network.rs"]
mod network;
use network::{NetworkAction, NetworkState};
#[path = "zaman-menu/audio.rs"]
mod audio;
use audio::{AudioAction, AudioOutput, AudioSnapshot, AudioState};
#[path = "zaman-menu/bluetooth.rs"]
mod bluetooth;
use bluetooth::{BluetoothAction, BluetoothClient, BluetoothEvent, BluetoothState, PairPrompt, PromptKind};
#[cfg(test)]
use audio::parse_volume;
#[path = "zaman-menu/display.rs"]
mod display;
use display::{DisplayAction, DisplayControls};
use zaman_sessiond::display::DisplayState;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const PRIMARY: &[u8] =
    include_bytes!("../../assets/fonts/saira-condensed/SairaCondensed-Regular.ttf");
const SECONDARY: &[u8] = include_bytes!("../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");
const JET: Color = Color::RGB(16, 12, 6);
const OBSIDIAN: Color = Color::RGB(27, 22, 16);
const SAND: Color = Color::RGB(230, 178, 90);
const BONE: Color = Color::RGB(242, 236, 224);
const CARAMEL: Color = Color::RGB(196, 117, 66);
const CONNECTED: Color = Color::RGB(115, 213, 149);
const DISCONNECTED: Color = Color::RGB(255, 139, 139);
const WARNING: Color = Color::RGB(244, 100, 87);

// Design space is 1920x1080. Every literal below is expressed in these
// units and scaled at render time to the actual output size, so glyphs
// rasterize at native resolution and stay crisp at 720p, 800p, 1080p,
// 1440p, and 4K. Non-16:9 outputs get centered letterbox / pillarbox.
const DESIGN_WIDTH: f32 = 1920.0;
const DESIGN_HEIGHT: f32 = 1080.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuCommand {
    Close,
    ExitGame,
    Transfer,
    Shutdown,
    Reboot,
    PowerYes,
    PowerNo,
    Volume,
    AudioOutput,
    AudioLevel,
    AudioApply,
    Mute,
    System,
    Network,
    Bluetooth,
    BluetoothPower,
    BluetoothDevices,
    BluetoothScan,
    BluetoothDevice(usize),
    BluetoothPair,
    BluetoothConnect,
    BluetoothDisconnect,
    BluetoothForget,
    BluetoothForgetConfirm,
    BluetoothPage,
    BluetoothAccept,
    BluetoothCancel,
    BluetoothWait,
    Display,
    DisplayOutput,
    DisplayResolution,
    DisplayRefresh,
    DisplayApply,
    DisplayKeep,
    DisplayRevert,
    DisplayRead,
    Wifi,
    Networks,
    Refresh,
    Disconnect,
    AccessPoint(usize),
    Page,
    KeyboardRow(usize),
    Back,
}
impl MenuCommand {
    fn action(self) -> &'static str {
        match self {
            Self::Close => "resume",
            Self::ExitGame => "exit-game",
            Self::Transfer => "transfer",
            Self::Shutdown => "shutdown",
            Self::Reboot => "reboot",
            _ => "",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::Close => "CloseMenu",
            Self::ExitGame => "ExitGame",
            Self::Transfer => "StartTransfer",
            Self::Shutdown => "Shutdown",
            Self::Reboot => "Reboot",
            _ => "Local",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct MenuItem {
    label: String,
    detail: String,
    command: MenuCommand,
}
fn item(label: impl Into<String>, detail: impl Into<String>, command: MenuCommand) -> MenuItem {
    MenuItem {
        label: label.into(),
        detail: detail.into(),
        command,
    }
}
fn items(context: &MenuContextTuple) -> Vec<MenuItem> {
    let mut result: Vec<MenuItem> = context
        .6
        .iter()
        .filter_map(|action| match action.as_str() {
            // B and Guide already dismiss the menu; no duplicate Resume cell.
            "resume" => None,
            "exit-game" => Some(item(
                "Exit Game",
                "Close the current game session",
                MenuCommand::ExitGame,
            )),
            "transfer" => Some(item(
                "Transfer Games",
                "Add games from your phone or PC",
                MenuCommand::Transfer,
            )),
            "reboot" => Some(item(
                "Reboot",
                if context.2 == "game" {
                    "Exit game and restart Zaman."
                } else {
                    "Restart Zaman."
                },
                MenuCommand::Reboot,
            )),
            "shutdown" => Some(item(
                "Shut Down",
                if context.2 == "game" { "Exit game and power off." } else { "Power off Zaman." },
                MenuCommand::Shutdown,
            )),
            _ => None,
        })
        .collect();
    if !result.is_empty() {
        result.insert(
            0,
            item("System", "Audio, network, Bluetooth and display settings", MenuCommand::System),
        );
    }
    result
}

#[derive(Debug, PartialEq)]
enum ModelEffect {
    None,
    Redraw,
    Activate(MenuCommand),
    Audio(AudioAction),
    Network(NetworkAction),
    Display(DisplayAction),
    Bluetooth(BluetoothAction),
    BluetoothReply(Option<String>),
    BluetoothCancel,
}
#[derive(Debug)]
enum NetworkRequest {
    Status,
    Scan,
    Change,
    Saved(String),
}
const SAVED_PROFILE_FAILED: &str =
    "Saved connection failed. Select it again to enter a password.";
const NETWORK_STATUS_FAILED: &str = "Network status unavailable. Check NetworkManager.";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuPage {
    Root,
    RebootConfirm,
    ShutdownConfirm,
    System,
    Volume,
    Network,
    Bluetooth,
    BluetoothDevices,
    BluetoothDevice,
    BluetoothPair,
    BluetoothPin,
    BluetoothForget,
    Display,
    AccessPoints,
    Password,
}

const LOWER_KEYS: [&[u8]; 4] = [b"qwertyuiop", b"asdfghjkl", b"zxcvbnm", b"1234567890"];
const UPPER_KEYS: [&[u8]; 4] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNM", b"1234567890"];
const SYMBOL_KEYS: [&[u8]; 4] = [b"!@#$%^&*()", b"-_=+[]{}\\|", b";:'\",.<>/?", b"1234567890"];

#[derive(Debug)]
struct MenuModel {
    generation: u64,
    selected: usize,
    suspended: bool,
    audio: Option<AudioState>,
    audio_outputs: Vec<AudioOutput>,
    audio_default: Option<String>,
    audio_selected: Option<String>,
    audio_busy: bool,
    audio_editing: bool,
    pressed: BTreeSet<String>,
    items: Vec<MenuItem>,
    root_items: Vec<MenuItem>,
    page: MenuPage,
    network: Option<NetworkState>,
    bluetooth: Option<BluetoothState>,
    bluetooth_busy: bool,
    bluetooth_changing: bool,
    bluetooth_operation: u64,
    bluetooth_cancelled: bool,
    bluetooth_page: usize,
    bluetooth_device: Option<bluetooth::Device>,
    bluetooth_prompt: Option<PairPrompt>,
    display: DisplayControls,
    network_busy: bool,
    scan_busy: bool,
    status_busy: bool,
    network_revision: u64,
    rescan_needed: bool,
    retry_password: Option<String>,
    ap_page: usize,
    pending_ssid: Option<String>,
    password: String,
    password_col: usize,
    password_layer: usize,
    password_visible: bool,
}
impl MenuModel {
    fn new(context: &MenuContextTuple) -> Self {
        Self {
            generation: context.1,
            selected: 0,
            suspended: context.4,
            audio: None,
            audio_outputs: Vec::new(),
            audio_default: None,
            audio_selected: None,
            audio_busy: false,
            audio_editing: false,
            pressed: BTreeSet::new(),
            items: items(context),
            root_items: items(context),
            page: MenuPage::Root,
            network: None,
            bluetooth: None,
            bluetooth_busy: false,
            bluetooth_changing: false,
            bluetooth_operation: 0,
            bluetooth_cancelled: false,
            bluetooth_page: 0,
            bluetooth_device: None,
            bluetooth_prompt: None,
            display: DisplayControls::default(),
            network_busy: false,
            scan_busy: false,
            status_busy: false,
            network_revision: 0,
            rescan_needed: false,
            retry_password: None,
            ap_page: 0,
            pending_ssid: None,
            password: String::new(),
            password_col: 0,
            password_layer: 0,
            password_visible: false,
        }
    }
    fn update(&mut self, context: &MenuContextTuple) {
        let root = items(context);
        let next = if self.page == MenuPage::Root {
            root.clone()
        } else {
            self.items.clone()
        };
        if self.suspended != context.4 || self.items != next {
            self.pressed.clear();
        }
        self.suspended = context.4;
        self.items = next;
        self.root_items = root;
        if self.power_action().is_some_and(|action| !self.root_items.iter().any(|item| item.command == action)) {
            self.set_page(MenuPage::Root);
        }
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }
    fn power_action(&self) -> Option<MenuCommand> {
        match self.page {
            MenuPage::RebootConfirm => Some(MenuCommand::Reboot),
            MenuPage::ShutdownConfirm => Some(MenuCommand::Shutdown),
            _ => None,
        }
    }
    fn cancel_power(&mut self) -> ModelEffect {
        let action = self.power_action();
        self.set_page(MenuPage::Root);
        self.selected = self.items.iter().position(|item| Some(item.command) == action).unwrap_or(0);
        ModelEffect::Redraw
    }
    fn bluetooth_page(&self) -> bool {
        matches!(self.page, MenuPage::Bluetooth | MenuPage::BluetoothDevices | MenuPage::BluetoothDevice
            | MenuPage::BluetoothPair | MenuPage::BluetoothPin | MenuPage::BluetoothForget)
    }
    fn password_page(&self) -> bool { matches!(self.page, MenuPage::Password | MenuPage::BluetoothPin) }
    fn password_limit(&self) -> usize {
        if self.page == MenuPage::BluetoothPin {
            if self.bluetooth_prompt.as_ref().is_some_and(|prompt| matches!(prompt.kind, PromptKind::Passkey)) { 6 } else { 16 }
        } else { 63 }
    }
    fn set_page(&mut self, page: MenuPage) {
        self.page = page;
        self.audio_editing = false;
        self.selected = 0;
        if page == MenuPage::Volume {
            self.selected = 1;
            self.audio_selected = self.audio_default.clone()
                .or_else(|| self.audio_outputs.first().map(|output| output.name.clone()));
        }
        if matches!(page, MenuPage::BluetoothForget | MenuPage::BluetoothPair) { self.selected = 1; }
        if self.power_action().is_some() { self.selected = 1; }
        if self.password_page() {
            self.password_col = 0;
            self.password_layer = 0;
            self.password_visible = false;
        }
        self.pressed.clear();
        self.rebuild();
    }
    fn password_keys(&self, row: usize) -> &'static [u8] {
        let layout = match self.password_layer {
            1 => &UPPER_KEYS,
            2 => &SYMBOL_KEYS,
            _ => &LOWER_KEYS,
        };
        layout[row]
    }
    fn password_width(&self) -> usize {
        if self.selected == 4 {
            5
        } else {
            self.password_keys(self.selected).len()
        }
    }
    fn cycle_password_mode(&mut self) -> ModelEffect {
        self.password_layer = (self.password_layer + 1) % 3;
        ModelEffect::Redraw
    }
    fn password_accept(&mut self) -> ModelEffect {
        if self.selected < 4 {
            if self.password.len() < self.password_limit() {
                self.password
                    .push(self.password_keys(self.selected)[self.password_col] as char);
            }
            return ModelEffect::Redraw;
        }
        match self.password_col {
            0 => {
                if self.password.len() < self.password_limit() {
                    self.password.push(' ');
                }
                ModelEffect::Redraw
            }
            1 => {
                self.password.pop();
                ModelEffect::Redraw
            }
            2 => self.cycle_password_mode(),
            3 => {
                self.password_visible = !self.password_visible;
                ModelEffect::Redraw
            }
            4 if self.page == MenuPage::BluetoothPin => {
                if self.bluetooth_prompt.as_ref().is_some_and(|prompt| prompt.valid_entry(&self.password)) {
                    ModelEffect::BluetoothReply(Some(self.password.clone()))
                } else { ModelEffect::None }
            }
            4 if !self.network_busy && (8..=63).contains(&self.password.len()) => {
                let Some(ssid) = self.pending_ssid.clone() else {
                    return ModelEffect::None;
                };
                let Some(device) = self.network.as_ref().and_then(|n| n.wifi_device.clone()) else {
                    return ModelEffect::None;
                };
                ModelEffect::Network(NetworkAction::Connect {
                    ssid,
                    device,
                    password: Some(std::mem::take(&mut self.password)),
                })
            }
            _ => ModelEffect::None,
        }
    }
    fn rebuild(&mut self) {
        self.items = match self.page {
            MenuPage::Root => self.root_items.clone(),
            MenuPage::RebootConfirm | MenuPage::ShutdownConfirm => vec![
                item("Yes", if self.page == MenuPage::RebootConfirm { "Restart Zaman." } else { "Power off Zaman." }, MenuCommand::PowerYes),
                item("No", "Return to the menu.", MenuCommand::PowerNo),
            ],
            MenuPage::System => vec![
                item(
                    "Volume",
                    "Choose a playback output, adjust volume and mute audio.",
                    MenuCommand::Volume,
                ),
                item(
                    "Network",
                    "View connections and configure Wi-Fi.",
                    MenuCommand::Network,
                ),
                item("Bluetooth", "Pair devices and manage Bluetooth connections.", MenuCommand::Bluetooth),
                item("Display", "Resolution and refresh rate settings.", MenuCommand::Display),
                item("Back", "Return to the main menu.", MenuCommand::Back),
            ],
            MenuPage::Volume => vec![
                item(
                    format!("Output: {}", self.selected_audio_output()
                        .map(|output| output.description.as_str()).unwrap_or("Unavailable")),
                    "Left / Right chooses an output. Select Apply output to use it.",
                    MenuCommand::AudioOutput,
                ),
                item(
                    "Volume",
                    if self.audio_editing {
                        if self.audio.is_some_and(|audio| audio.muted) {
                            "Muted level. Left / Right changes it. A confirms. B done."
                        } else { "Left / Right changes volume. A confirms and returns. B done." }
                    } else { "A adjusts volume. Right selects mute. Up / Down changes rows." },
                    MenuCommand::AudioLevel,
                ),
                item(
                    if self.audio.is_some_and(|audio| audio.muted) { "Unmute" } else { "Mute" },
                    if self.audio.is_some_and(|audio| audio.muted) {
                        "A unmutes audio. Left returns to the slider. Your volume level is preserved."
                    } else {
                        "A mutes audio. Left returns to the slider. Your volume level is preserved."
                    },
                    MenuCommand::Mute,
                ),
                item(
                    "Apply output",
                    "Use the selected playback output. Its own volume and mute state will be shown.",
                    MenuCommand::AudioApply,
                ),
                item("Back", "Return to System. Volume changes are already applied.", MenuCommand::Back),
            ],
            MenuPage::Bluetooth => vec![
                item(match self.bluetooth.as_ref() {
                    None => "Bluetooth  Checking...",
                    Some(state) => match state.adapter.as_ref() {
                        Some(adapter) if adapter.powered => "Bluetooth  On",
                        Some(_) => "Bluetooth  Off",
                        None => "Bluetooth  No adapter",
                    },
                },
                    "Turn Bluetooth on or off.", MenuCommand::BluetoothPower),
                item("Devices", "View known devices and their pairing/connection status.", MenuCommand::BluetoothDevices),
                item(if self.bluetooth.as_ref().is_some_and(|state| state.own_scan) { "Stop discovery" } else { "Find devices" },
                    "Put the other device in pairing mode. Discovery runs for 30 seconds.", MenuCommand::BluetoothScan),
                item("Back", "Return to System.", MenuCommand::Back),
            ],
            MenuPage::BluetoothDevices => {
                let devices = self.bluetooth.as_ref().map(|state| state.devices.as_slice()).unwrap_or_default();
                let mut rows: Vec<_> = devices.iter().enumerate().skip(self.bluetooth_page * 3).take(3)
                    .map(|(index, device)| item(&device.name,
                        format!("{} / {}{}", device.address, if device.paired { "Paired" } else { "Not paired" },
                            device.battery.map(|level| format!(" / Battery {level}%")).unwrap_or_default()),
                        MenuCommand::BluetoothDevice(index))).collect();
                if rows.is_empty() { rows.push(item("No devices yet", "Turn Bluetooth on, then use Find devices. Put the other device in pairing mode.", MenuCommand::BluetoothWait)); }
                if devices.len() > 3 {
                    rows.push(item(format!("Page {} / {}", self.bluetooth_page + 1, devices.len().div_ceil(3)),
                        "Left / Right or A changes the page.", MenuCommand::BluetoothPage));
                }
                rows.push(item("Back", "Return to Bluetooth.", MenuCommand::Back));
                rows
            }
            MenuPage::BluetoothDevice => {
                let device = self.bluetooth_device.as_ref();
                let mut rows = Vec::new();
                if device.is_some_and(|device| !device.paired) {
                    rows.push(item("Pair device", "Pair and remember this device. Confirm any code shown on both devices.", MenuCommand::BluetoothPair));
                }
                rows.push(item(if device.is_some_and(|device| device.connected) { "Disconnect" } else { "Connect" },
                    "Use this device's Bluetooth services. Pair it first if needed.",
                    if device.is_some_and(|device| device.connected) { MenuCommand::BluetoothDisconnect } else { MenuCommand::BluetoothConnect }));
                if device.is_some_and(|device| device.paired) {
                    rows.push(item("Forget device", "Remove the pairing after confirmation. You will need to pair again.", MenuCommand::BluetoothForget));
                }
                rows.push(item("Back", "Return to Devices.", MenuCommand::Back));
                rows
            }
            MenuPage::BluetoothPair => {
                let mut rows = Vec::new();
                if self.bluetooth_prompt.as_ref().is_some_and(|prompt| matches!(prompt.kind, PromptKind::Confirm(_) | PromptKind::Authorize)) {
                    rows.push(item("Confirm pairing", "Confirm only the device and code you requested.", MenuCommand::BluetoothAccept));
                } else {
                    rows.push(item("Pairing...", "Follow the instructions on the right and on the other device.", MenuCommand::BluetoothWait));
                }
                rows.push(item("Cancel pairing", "Cancel this pairing request.", MenuCommand::BluetoothCancel));
                rows
            }
            MenuPage::BluetoothForget => vec![
                item("Forget device", "Remove its pairing information. Pair again before reconnecting.", MenuCommand::BluetoothForgetConfirm),
                item("Cancel", "Keep this device paired.", MenuCommand::Back),
            ],
            MenuPage::Display => self.display.rows(),
            MenuPage::Network => vec![
                item(
                    if self.network.as_ref().is_some_and(|n| n.wifi_enabled) {
                        "Wi-Fi  On"
                    } else {
                        "Wi-Fi  Off"
                    },
                    "Turn Wi-Fi on or off.",
                    MenuCommand::Wifi,
                ),
                item(
                    "Available Networks",
                    "Scan and join Wi-Fi networks.",
                    MenuCommand::Networks,
                ),
                item(
                    "Refresh",
                    "Update Ethernet, Wi-Fi and IP address status.",
                    MenuCommand::Refresh,
                ),
                item(
                    "Disconnect Wi-Fi",
                    "Disconnect the current wireless connection.",
                    MenuCommand::Disconnect,
                ),
                item("Back", "Return to System.", MenuCommand::Back),
            ],
            MenuPage::AccessPoints => {
                let aps = self
                    .network
                    .as_ref()
                    .map(|n| n.access_points.as_slice())
                    .unwrap_or(&[]);
                let mut rows: Vec<MenuItem> = aps
                    .iter()
                    .enumerate()
                    .skip(self.ap_page * 3)
                    .take(3)
                    .map(|(i, ap)| {
                        item(
                            format!(
                                "{}{}",
                                if ap.active { "* " } else { "" },
                                ap.ssid
                                    .chars()
                                    .filter(|c| !c.is_control())
                                    .take(22)
                                    .collect::<String>()
                            ),
                            format!(
                                "{} / {}% signal{}{}",
                                if ap.security.is_empty() || ap.security == "--" {
                                    "Open"
                                } else {
                                    &ap.security
                                },
                                ap.signal,
                                if ap.active { " / Connected" } else { "" },
                                if !ap.active
                                    && self.network.as_ref().is_some_and(|n| {
                                        n.saved_profiles.contains_key(&ap.ssid)
                                    })
                                {
                                    " / Saved"
                                } else {
                                    ""
                                }
                            ),
                            MenuCommand::AccessPoint(i),
                        )
                    })
                    .collect();
                if aps.len() > 3 {
                    rows.push(item(
                        format!("Page {} / {}", self.ap_page + 1, aps.len().div_ceil(3)),
                        "Left / Right to change page; A for next.",
                        MenuCommand::Page,
                    ));
                }
                if rows.is_empty() {
                    rows.push(item(
                        if self.scan_busy {
                            "Scanning..."
                        } else {
                            "No networks"
                        },
                        if self.scan_busy {
                            "Searching for Wi-Fi networks."
                        } else {
                            "Select Back, then Available Networks to scan again."
                        },
                        MenuCommand::Networks,
                    ));
                }
                rows.push(item("Back", "Return to Network.", MenuCommand::Back));
                rows
            }
            MenuPage::Password | MenuPage::BluetoothPin => (0..5)
                .map(|row| {
                    item(
                        "",
                        "Choose a key with the D-pad; press A.",
                        MenuCommand::KeyboardRow(row),
                    )
                })
                .collect(),
        };
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }
    fn selected_audio_output(&self) -> Option<&AudioOutput> {
        self.audio_outputs.iter().find(|output| Some(&output.name) == self.audio_selected.as_ref())
    }
    fn update_audio(&mut self, snapshot: AudioSnapshot) {
        let default_changed = self.audio_default != snapshot.default_name;
        if self.audio_editing && (default_changed || snapshot.level.is_none()) { self.audio_editing = false; self.pressed.clear(); }
        let candidate = if default_changed { snapshot.default_name.clone() } else { self.audio_selected.clone() };
        self.audio_selected = candidate.filter(|name| snapshot.outputs.iter().any(|output| &output.name == name))
            .or_else(|| snapshot.current().map(|output| output.name.clone()))
            .or_else(|| snapshot.outputs.first().map(|output| output.name.clone()));
        self.audio = snapshot.level;
        self.audio_outputs = snapshot.outputs;
        self.audio_default = snapshot.default_name;
        self.rebuild();
    }
    fn cycle_audio_output(&mut self, forward: bool) -> ModelEffect {
        if self.audio_busy || self.audio_outputs.is_empty() { return ModelEffect::None; }
        let current = self.audio_outputs.iter().position(|output| Some(&output.name) == self.audio_selected.as_ref()).unwrap_or(0);
        let count = self.audio_outputs.len();
        let next = if forward { (current + 1) % count } else { (current + count - 1) % count };
        self.audio_selected = Some(self.audio_outputs[next].name.clone());
        self.rebuild();
        ModelEffect::Redraw
    }
    fn update_bluetooth(&mut self, state: BluetoothState) {
        let selected = if self.page == MenuPage::BluetoothDevices {
            self.items.get(self.selected).and_then(|item| match item.command {
                MenuCommand::BluetoothDevice(index) => self.bluetooth.as_ref()?.devices.get(index).map(|device| (device.adapter.clone(), device.address.clone())),
                _ => None,
            })
        } else { None };
        if let Some(device) = &self.bluetooth_device {
            self.bluetooth_device = state.devices.iter().find(|next| next.address == device.address && next.adapter == device.adapter).cloned();
        }
        let mut selected_removed = false;
        if let Some((adapter, address)) = selected {
            if let Some(index) = state.devices.iter().position(|device| device.adapter == adapter && device.address == address) {
                self.bluetooth_page = index / 3;
                self.selected = index % 3;
            } else { selected_removed = true; }
        }
        self.bluetooth_page = self.bluetooth_page.min(state.devices.len().div_ceil(3).saturating_sub(1));
        self.bluetooth = Some(state);
        if self.bluetooth_device.is_none() && matches!(self.page, MenuPage::BluetoothDevice | MenuPage::BluetoothForget) {
            self.set_page(MenuPage::BluetoothDevices);
            self.selected = self.items.len().saturating_sub(1);
        } else { self.rebuild(); }
        if selected_removed { self.selected = self.items.len().saturating_sub(1); self.pressed.clear(); }
    }
    fn bluetooth_page_step(&mut self, forward: bool) -> ModelEffect {
        let count = self.bluetooth.as_ref().map(|state| state.devices.len().div_ceil(3)).unwrap_or(0);
        if count == 0 { return ModelEffect::None; }
        self.bluetooth_page = if forward { (self.bluetooth_page + 1) % count } else { (self.bluetooth_page + count - 1) % count };
        self.rebuild();
        ModelEffect::Redraw
    }
    fn update_network(&mut self, mut state: NetworkState, scanned: bool) -> bool {
        if !scanned && state.wifi_enabled && state.wifi_device.is_some() {
            if let Some(previous) = &self.network {
                state.access_points = previous.access_points.clone();
                state.saved_profiles = previous.saved_profiles.clone();
            }
        }
        // Cached rows retain signal/security data, but their connection badges
        // follow the latest status read, including renamed profiles/disconnects.
        for ap in &mut state.access_points {
            ap.active = state.wifi_enabled && state.wifi_connection.is_some()
                && state.wifi_ssid.as_deref() == Some(ap.ssid.as_str());
        }
        if self.network.as_ref() == Some(&state) {
            return false;
        }
        let selected_command = if scanned && self.page == MenuPage::AccessPoints {
            self.items.get(self.selected).map(|item| item.command)
        } else {
            None
        };
        let selected_ssid = if scanned && self.page == MenuPage::AccessPoints {
            self.items
                .get(self.selected)
                .and_then(|item| match item.command {
                    MenuCommand::AccessPoint(i) => self
                        .network
                        .as_ref()?
                        .access_points
                        .get(i)
                        .map(|ap| ap.ssid.clone()),
                    _ => None,
                })
        } else {
            None
        };
        self.network = Some(state);
        let pages = self.network.as_ref().unwrap().access_points.len().div_ceil(3);
        self.ap_page = self.ap_page.min(pages.saturating_sub(1));
        let mut selected_network_disappeared = false;
        if let Some(ssid) = &selected_ssid {
            if let Some(position) = self
                .network
                .as_ref()
                .unwrap()
                .access_points
                .iter()
                .position(|ap| ap.ssid == ssid.as_str())
            {
                self.ap_page = position / 3;
                self.selected = position % 3;
            } else {
                selected_network_disappeared = true;
            }
        }
        self.rebuild();
        if selected_network_disappeared {
            if let Some(position) = self.items.iter().position(|item| item.command == MenuCommand::Back) {
                self.selected = position;
            }
        } else if selected_ssid.is_none() {
            if let Some(command) = selected_command {
                if matches!(command, MenuCommand::Back | MenuCommand::Page) {
                    if let Some(position) = self.items.iter().position(|item| item.command == command) {
                        self.selected = position;
                    }
                }
            }
        }
        true
    }
    fn selected_command(&mut self) -> ModelEffect {
        if self.password_page() {
            return self.password_accept();
        }
        let Some(command) = self.items.get(self.selected).map(|item| item.command) else {
            return ModelEffect::None;
        };
        match command {
            MenuCommand::Reboot | MenuCommand::Shutdown => {
                self.set_page(if command == MenuCommand::Reboot { MenuPage::RebootConfirm } else { MenuPage::ShutdownConfirm });
                ModelEffect::Redraw
            }
            MenuCommand::PowerYes => self.power_action()
                .filter(|action| self.root_items.iter().any(|item| item.command == *action))
                .map(ModelEffect::Activate).unwrap_or(ModelEffect::None),
            MenuCommand::PowerNo => self.cancel_power(),
            MenuCommand::Bluetooth => {
                self.set_page(MenuPage::Bluetooth);
                ModelEffect::Bluetooth(BluetoothAction::Read)
            }
            MenuCommand::BluetoothDevices => {
                self.set_page(MenuPage::BluetoothDevices);
                ModelEffect::Bluetooth(BluetoothAction::Read)
            }
            MenuCommand::BluetoothPower | MenuCommand::BluetoothScan if !self.bluetooth_busy => {
                let Some(state) = &self.bluetooth else { return ModelEffect::None; };
                let Some(adapter) = &state.adapter else { return ModelEffect::None; };
                if command == MenuCommand::BluetoothPower {
                    ModelEffect::Bluetooth(BluetoothAction::Power { adapter: adapter.path.clone(), enabled: !adapter.powered })
                } else if adapter.powered {
                    let action = BluetoothAction::Scan { adapter: adapter.path.clone(), start: !state.own_scan };
                    if !state.own_scan { self.set_page(MenuPage::BluetoothDevices); }
                    ModelEffect::Bluetooth(action)
                } else { ModelEffect::None }
            }
            MenuCommand::BluetoothDevice(index) => {
                self.bluetooth_device = self.bluetooth.as_ref().and_then(|state| state.devices.get(index)).cloned();
                if self.bluetooth_device.is_none() { return ModelEffect::None; }
                self.set_page(MenuPage::BluetoothDevice);
                ModelEffect::Redraw
            }
            MenuCommand::BluetoothPair | MenuCommand::BluetoothConnect | MenuCommand::BluetoothDisconnect
                | MenuCommand::BluetoothForgetConfirm if !self.bluetooth_busy => {
                let Some(device) = self.bluetooth_device.clone() else { return ModelEffect::None; };
                let action = match command {
                    MenuCommand::BluetoothPair => {
                        if device.paired { return ModelEffect::None; }
                        self.bluetooth_prompt = None;
                        self.set_page(MenuPage::BluetoothPair);
                        BluetoothAction::Pair(device)
                    }
                    MenuCommand::BluetoothConnect if device.paired => BluetoothAction::Connect(device),
                    MenuCommand::BluetoothDisconnect => BluetoothAction::Disconnect(device),
                    MenuCommand::BluetoothForgetConfirm => BluetoothAction::Forget(device),
                    _ => return ModelEffect::None,
                };
                ModelEffect::Bluetooth(action)
            }
            MenuCommand::BluetoothForget if !self.bluetooth_busy => {
                self.set_page(MenuPage::BluetoothForget);
                ModelEffect::Redraw
            }
            MenuCommand::BluetoothPage => self.bluetooth_page_step(true),
            MenuCommand::BluetoothAccept => ModelEffect::BluetoothReply(Some(String::new())),
            MenuCommand::BluetoothCancel => ModelEffect::BluetoothCancel,
            MenuCommand::BluetoothPower | MenuCommand::BluetoothScan | MenuCommand::BluetoothPair
                | MenuCommand::BluetoothConnect | MenuCommand::BluetoothDisconnect | MenuCommand::BluetoothForget
                | MenuCommand::BluetoothForgetConfirm | MenuCommand::BluetoothWait => ModelEffect::None,
            MenuCommand::Display => {
                self.set_page(MenuPage::Display);
                ModelEffect::Display(DisplayAction::Read)
            }
            MenuCommand::DisplayOutput | MenuCommand::DisplayResolution | MenuCommand::DisplayRefresh => {
                self.display.cycle(command, true);
                self.rebuild();
                ModelEffect::Redraw
            }
            MenuCommand::DisplayApply | MenuCommand::DisplayKeep | MenuCommand::DisplayRevert | MenuCommand::DisplayRead => {
                self.display.select(command).map(ModelEffect::Display).unwrap_or(ModelEffect::None)
            }
            MenuCommand::Volume => {
                self.set_page(MenuPage::Volume);
                if self.audio_busy { ModelEffect::Redraw } else { ModelEffect::Audio(AudioAction::Read) }
            }
            MenuCommand::AudioOutput => self.cycle_audio_output(true),
            MenuCommand::AudioApply if !self.audio_busy => {
                self.selected_audio_output()
                    .filter(|output| Some(&output.name) != self.audio_default.as_ref())
                    .cloned().map(|output| ModelEffect::Audio(AudioAction::SetDefault(output)))
                    .unwrap_or(ModelEffect::None)
            }
            MenuCommand::AudioApply => ModelEffect::None,
            MenuCommand::AudioLevel if !self.audio_busy && self.audio.is_some() => {
                if self.audio_editing { self.set_page(MenuPage::System); }
                else { self.audio_editing = true; self.pressed.clear(); self.rebuild(); }
                ModelEffect::Redraw
            }
            MenuCommand::AudioLevel => ModelEffect::None,
            MenuCommand::Mute if !self.audio_busy && self.audio.is_some() => {
                ModelEffect::Audio(AudioAction::SetMute(!self.audio.unwrap().muted))
            }
            MenuCommand::Mute => ModelEffect::None,
            MenuCommand::System => {
                self.set_page(MenuPage::System);
                ModelEffect::Redraw
            }
            MenuCommand::Network => {
                self.set_page(MenuPage::Network);
                ModelEffect::Network(NetworkAction::Read { scan: false })
            }
            MenuCommand::Wifi if !self.network_busy => ModelEffect::Network(NetworkAction::Wifi {
                enabled: !self.network.as_ref().is_some_and(|n| n.wifi_enabled),
                device: self.network.as_ref().and_then(|n| n.wifi_device.clone()),
            }),
            MenuCommand::Networks if !self.network_busy => {
                self.ap_page = 0;
                self.set_page(MenuPage::AccessPoints);
                ModelEffect::Network(NetworkAction::Read { scan: true })
            }
            MenuCommand::Refresh if !self.network_busy => {
                ModelEffect::Network(NetworkAction::Read { scan: false })
            }
            MenuCommand::Disconnect if !self.network_busy => self
                .network
                .as_ref()
                .filter(|n| n.wifi_connection.is_some())
                .and_then(|n| n.wifi_device.clone())
                .map(|device| ModelEffect::Network(NetworkAction::Disconnect(device)))
                .unwrap_or(ModelEffect::None),
            MenuCommand::AccessPoint(i) if !self.network_busy => {
                let Some(ap) = self.network.as_ref().and_then(|n| n.access_points.get(i)) else {
                    return ModelEffect::None;
                };
                if ap.active {
                    return ModelEffect::None;
                }
                if !ap.security.is_empty() && ap.security != "--" {
                    if ap.security.contains("802.1X") || ap.security.contains("WEP") {
                        return ModelEffect::None;
                    }
                    if self.retry_password.as_deref() != Some(ap.ssid.as_str()) {
                        if let Some(uuid) = self
                            .network
                            .as_ref()
                            .and_then(|n| n.saved_profiles.get(&ap.ssid))
                        {
                            if let Some(device) = self
                                .network
                                .as_ref()
                                .and_then(|n| n.wifi_device.as_ref())
                            {
                                return ModelEffect::Network(NetworkAction::Saved {
                                    ssid: ap.ssid.clone(),
                                    uuid: uuid.clone(),
                                    device: device.clone(),
                                });
                            }
                        }
                    }
                    self.retry_password = None;
                    self.pending_ssid = Some(ap.ssid.clone());
                    self.password.clear();
                    self.set_page(MenuPage::Password);
                    return ModelEffect::Redraw;
                }
                self.network
                    .as_ref()
                    .and_then(|n| n.wifi_device.clone())
                    .map(|device| {
                        ModelEffect::Network(NetworkAction::Connect {
                            ssid: ap.ssid.clone(),
                            device,
                            password: None,
                        })
                    })
                    .unwrap_or(ModelEffect::None)
            }
            MenuCommand::Page => {
                self.page_step(true);
                ModelEffect::Redraw
            }
            MenuCommand::Back => self.back(),
            command if command.action().is_empty() => ModelEffect::None,
            command => ModelEffect::Activate(command),
        }
    }
    fn back(&mut self) -> ModelEffect {
        match self.page {
            MenuPage::RebootConfirm | MenuPage::ShutdownConfirm => self.cancel_power(),
            MenuPage::Bluetooth => { self.set_page(MenuPage::System); ModelEffect::Redraw }
            MenuPage::BluetoothDevices => { self.set_page(MenuPage::Bluetooth); ModelEffect::Redraw }
            MenuPage::BluetoothDevice => { self.set_page(MenuPage::BluetoothDevices); ModelEffect::Redraw }
            MenuPage::BluetoothForget => { self.set_page(MenuPage::BluetoothDevice); ModelEffect::Redraw }
            MenuPage::BluetoothPair | MenuPage::BluetoothPin => ModelEffect::BluetoothCancel,
            MenuPage::Volume => {
                if self.audio_editing { self.audio_editing = false; self.pressed.clear(); self.rebuild(); }
                else { self.set_page(MenuPage::System); }
                ModelEffect::Redraw
            }
            MenuPage::Display => {
                if self.display.busy { return ModelEffect::None }
                if let Some(action) = self.display.select(MenuCommand::Back) {
                    return ModelEffect::Display(action);
                }
                self.set_page(MenuPage::System);
                ModelEffect::Redraw
            }
            MenuPage::Root => ModelEffect::Activate(MenuCommand::Close),
            MenuPage::System => {
                self.set_page(MenuPage::Root);
                ModelEffect::Redraw
            }
            MenuPage::Network => {
                self.set_page(MenuPage::System);
                ModelEffect::Redraw
            }
            MenuPage::AccessPoints => {
                self.set_page(MenuPage::Network);
                ModelEffect::Redraw
            }
            MenuPage::Password => {
                self.password.clear();
                self.password_visible = false;
                self.pending_ssid = None;
                self.set_page(MenuPage::AccessPoints);
                ModelEffect::Redraw
            }
        }
    }
    fn page_step(&mut self, forward: bool) {
        let count = self
            .network
            .as_ref()
            .map(|n| n.access_points.len().div_ceil(3))
            .unwrap_or(0);
        if count > 0 {
            self.ap_page = if forward {
                (self.ap_page + 1) % count
            } else {
                (self.ap_page + count - 1) % count
            };
            self.rebuild();
        }
    }
    fn horizontal(&mut self, right: bool) -> ModelEffect {
        if self.items.get(self.selected).is_some_and(|item| item.command == MenuCommand::BluetoothPage) {
            return self.bluetooth_page_step(right);
        }
        if self.page == MenuPage::Volume && !self.audio_editing
            && self.items.get(self.selected).is_some_and(|item| item.command == MenuCommand::AudioLevel) {
            if right { self.selected = 2; return ModelEffect::Redraw; }
            return ModelEffect::None;
        }
        if self.page == MenuPage::Volume
            && self.items.get(self.selected).is_some_and(|item| item.command == MenuCommand::Mute) {
            if !right {
                self.selected = self.items.iter().position(|item| item.command == MenuCommand::AudioLevel).unwrap_or(self.selected);
                return ModelEffect::Redraw;
            }
            return ModelEffect::None;
        }
        if self.items.get(self.selected).is_some_and(|item| item.command == MenuCommand::AudioOutput) {
            return self.cycle_audio_output(right);
        }
        if self.page == MenuPage::Display {
            if let Some(command) = self.items.get(self.selected).map(|i| i.command) {
                self.display.cycle(command, right);
                self.rebuild();
            }
            return ModelEffect::Redraw;
        }
        if self.page == MenuPage::AccessPoints
            && self
                .items
                .get(self.selected)
                .is_some_and(|item| item.command == MenuCommand::Page)
        {
            self.page_step(right);
            return ModelEffect::Redraw;
        }
        if self.password_page() {
            let count = self.password_width();
            self.password_col = if right {
                (self.password_col + 1) % count
            } else {
                (self.password_col + count - 1) % count
            };
            return ModelEffect::Redraw;
        }
        if self.audio_busy || !self.audio_editing
            || self.items.get(self.selected).map(|item| item.command) != Some(MenuCommand::AudioLevel)
        {
            return ModelEffect::None;
        }
        let Some(audio) = self.audio else {
            return ModelEffect::None;
        };
        let next = if right {
            audio.percent.saturating_add(5).min(100)
        } else {
            audio.percent.saturating_sub(5)
        };
        if next == audio.percent {
            ModelEffect::None
        } else {
            ModelEffect::Audio(AudioAction::Set(next))
        }
    }
    fn navigate(&mut self, up: bool) -> ModelEffect {
        if self.items.is_empty() {
            return ModelEffect::None;
        }
        if self.page == MenuPage::Volume {
            // The slider and icon occupy one visual row; vertical movement
            // skips the icon, while horizontal focus reaches it directly.
            let row = match self.selected { 0 => 0usize, 1 | 2 => 1, 3 => 2, _ => 3 };
            let next = if up { (row + 3) % 4 } else { (row + 1) % 4 };
            self.selected = [0, 1, 3, 4][next];
            if self.audio_editing { self.pressed.clear(); }
            self.audio_editing = false;
            self.rebuild();
            return ModelEffect::Redraw;
        }
        self.selected = if up {
            (self.selected + self.items.len() - 1) % self.items.len()
        } else {
            (self.selected + 1) % self.items.len()
        };
        if self.password_page() {
            self.password_col = self.password_col.min(self.password_width() - 1);
        }
        ModelEffect::Redraw
    }
    fn normalized_input(&mut self, event: &str, value: f64) -> ModelEffect {
        if self.suspended {
            return ModelEffect::None;
        }
        // Keep ALL interception for the complete click, as in v0.6.2.
        if value <= 0.5 {
            if !self.pressed.remove(event) {
                return ModelEffect::None;
            }
            return match event {
                "ui_accept" => self.selected_command(),
                "ui_back" | "ui_cancel" => self.back(),
                "ui_context" if self.password_page() => self.cycle_password_mode(),
                _ => ModelEffect::None,
            };
        }
        if !self.pressed.insert(event.to_string()) {
            return ModelEffect::None;
        }
        match event {
            "ui_up" => self.navigate(true),
            "ui_down" => self.navigate(false),
            "ui_left" => self.horizontal(false),
            "ui_right" => self.horizontal(true),
            _ => ModelEffect::None,
        }
    }
    fn keyboard_input(&mut self, keycode: Keycode) -> ModelEffect {
        if self.suspended {
            return ModelEffect::None;
        }
        match keycode {
            Keycode::Up => self.navigate(true),
            Keycode::Down => self.navigate(false),
            Keycode::Left => self.horizontal(false),
            Keycode::Right => self.horizontal(true),
            Keycode::Return | Keycode::Space => self.selected_command(),
            Keycode::Escape => self.back(),
            Keycode::X if self.password_page() => self.cycle_password_mode(),
            _ => ModelEffect::None,
        }
    }
}

#[derive(Clone)]
struct Fonts {
    primary: Option<PathBuf>,
    secondary: Option<PathBuf>,
}
impl Fonts {
    fn new() -> Result<Self> {
        fn path(name: &str) -> Result<Option<PathBuf>> {
            let Some(path) = std::env::var_os(name).map(PathBuf::from) else {
                return Ok(None);
            };
            if !path.is_file() {
                return Err(
                    other(format!("{name} is not a regular file: {}", path.display())).into(),
                );
            }
            Ok(Some(path))
        }
        Ok(Self {
            primary: path("ZAMAN_MENU_FONT")?,
            secondary: path("ZAMAN_MENU_DETAIL_FONT")?,
        })
    }
    fn load<'a>(
        &self,
        ttf: &'a Sdl2TtfContext,
        size: u16,
        secondary: bool,
    ) -> Result<Font<'a, 'static>> {
        let path = if secondary {
            &self.secondary
        } else {
            &self.primary
        };
        if let Some(path) = path {
            return Ok(ttf.load_font(path, size).map_err(other)?);
        }
        let bytes = if secondary { SECONDARY } else { PRIMARY };
        Ok(ttf
            .load_font_from_rwops(RWops::from_bytes(bytes).map_err(other)?, size)
            .map_err(other)?)
    }
}

struct LoadedFonts {
    title: Font<'static, 'static>,
    label: Font<'static, 'static>,
    small: Font<'static, 'static>,
    detail: Font<'static, 'static>,
}

impl LoadedFonts {
    fn load(ttf: &'static Sdl2TtfContext, paths: &Fonts, scale: f32) -> Result<Self> {
        let sz = |v: f32| (v * scale).round().max(1.0) as u16;
        Ok(Self {
            title: paths.load(ttf, sz(108.0), false)?,
            label: paths.load(ttf, sz(57.0), false)?,
            small: paths.load(ttf, sz(24.0), true)?,
            detail: paths.load(ttf, sz(27.0), true)?,
        })
    }
}

// Action descriptions and status/error text live in the right-hand panel.
struct RowLayout {
    top: f32,
    height: f32,
    gap: f32,
}

impl RowLayout {
    fn new(count: usize) -> Self {
        if count > 4 {
            Self {
                top: 315.0,
                height: 108.0,
                gap: 12.0,
            }
        } else if count > 3 {
            Self {
                top: 324.0,
                height: 126.0,
                gap: 18.0,
            }
        } else {
            Self {
                top: 369.0,
                height: 156.0,
                gap: 27.0,
            }
        }
    }

    fn y(&self, index: usize) -> f32 {
        self.top + index as f32 * (self.height + self.gap)
    }
}

struct MenuSurface {
    canvas: Canvas<Window>,
    model: MenuModel,
    context: MenuContextTuple,
    request_pending: bool,
    local_error: Option<String>,
    audio_error: Option<String>,
    bluetooth_error: Option<String>,
    bluetooth_reply: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    fonts: LoadedFonts,
    font_paths: Fonts,
    ttf: &'static Sdl2TtfContext,
    font_scale: f32,
}
impl MenuSurface {
    fn open(
        video: &sdl2::VideoSubsystem,
        ttf: &'static Sdl2TtfContext,
        paths: &Fonts,
        context: MenuContextTuple,
    ) -> Result<Self> {
        let window = video
            .window("Zaman System Menu", 1920, 1080)
            .resizable()
            .position_centered()
            .borderless()
            .allow_highdpi()
            .build()
            .map_err(other)?;
        let canvas = window
            .into_canvas()
            .present_vsync()
            .build()
            .map_err(other)?;
        let (width, height) = canvas.output_size().map_err(other)?;
        let scale = (width as f32 / DESIGN_WIDTH).min(height as f32 / DESIGN_HEIGHT);
        let fonts = LoadedFonts::load(ttf, paths, scale)?;
        Ok(Self {
            canvas,
            model: MenuModel::new(&context),
            context,
            request_pending: false,
            local_error: None,
            audio_error: None,
            bluetooth_error: None,
            bluetooth_reply: None,
            fonts,
            font_paths: paths.clone(),
            ttf,
            font_scale: scale,
        })
    }
    fn pending(&self) -> bool {
        self.request_pending || self.context.4
    }
    fn update(&mut self, context: MenuContextTuple) {
        self.model.update(&context);
        self.context = context;
    }
    fn input(&mut self, event: &str, value: f64) -> ModelEffect {
        if self.pending() {
            if value <= 0.5 {
                self.model.pressed.remove(event);
            }
            return ModelEffect::None;
        }
        self.model.normalized_input(event, value)
    }
    fn clear_bluetooth_prompt(&mut self) {
        if let Some(reply)=self.bluetooth_reply.take() { let _=reply.send(None); }
        self.model.bluetooth_prompt=None;
        if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) {
            self.model.password.clear(); self.model.password_visible=false;
        }
    }
    fn render_power_confirmation(&mut self, action: MenuCommand, width: u32, height: u32) -> Result<()> {
        let scale = self.font_scale;
        let offset_x = (width as f32 - DESIGN_WIDTH * scale) / 2.0;
        let offset_y = (height as f32 - DESIGN_HEIGHT * scale) / 2.0;
        let px = |v: f32| (offset_x + v * scale).round() as i32;
        let py = |v: f32| (offset_y + v * scale).round() as i32;
        let pw = |v: f32| (v * scale).round().max(1.0) as u32;
        let rect = |x, y, w, h| Rect::new(px(x), py(y), pw(w), pw(h));
        let reboot = action == MenuCommand::Reboot;
        self.canvas.set_draw_color(JET);
        self.canvas.clear();
        self.canvas.set_draw_color(OBSIDIAN);
        self.canvas.fill_rect(rect(96.0, 120.0, 1728.0, 804.0)).map_err(other)?;
        self.canvas.set_draw_color(WARNING);
        self.canvas.fill_rect(rect(96.0, 120.0, 12.0, 804.0)).map_err(other)?;
        self.canvas.fill_rect(rect(96.0, 120.0, 1728.0, 52.0)).map_err(other)?;
        for inset in 0..3 {
            self.canvas.draw_rect(rect(96.0 + inset as f32, 120.0 + inset as f32,
                1728.0 - inset as f32 * 2.0, 804.0 - inset as f32 * 2.0)).map_err(other)?;
        }
        draw_text(&mut self.canvas, &self.fonts.small, "ZAMAN / POWER CONFIRMATION", JET, px(156.0), py(130.0))?;
        draw_cell_icon(&mut self.canvas, rect(156.0, 228.0, 140.0, 140.0), action, WARNING)?;
        draw_text(&mut self.canvas, &self.fonts.title,
            if reboot { "REBOOT ZAMAN?" } else { "SHUT DOWN ZAMAN?" }, BONE, px(336.0), py(198.0))?;
        let consequence = match (reboot, self.context.2.as_str()) {
            (true, "game") => "This will close the running game and restart Zaman.",
            (false, "game") => "This will close the running game and power off Zaman.",
            (true, _) => "This will restart Zaman.",
            (false, _) => "This will power off Zaman.",
        };
        draw_wrapped(&mut self.canvas, &self.fonts.detail, consequence,
            BONE, px(336.0), py(354.0), pw(1350.0), pw(120.0))?;
        for (index, item) in self.model.items.iter().enumerate() {
            let selected = index == self.model.selected;
            let yes = item.command == MenuCommand::PowerYes;
            let accent = if yes { WARNING } else { SAND };
            let y = 510.0 + index as f32 * 156.0;
            self.canvas.set_draw_color(if selected { accent } else { JET });
            self.canvas.fill_rect(rect(156.0, y, 1608.0, 126.0)).map_err(other)?;
            self.canvas.set_draw_color(accent);
            for inset in 0..3 {
                self.canvas.draw_rect(rect(156.0 + inset as f32, y + inset as f32,
                    1608.0 - inset as f32 * 2.0, 126.0 - inset as f32 * 2.0)).map_err(other)?;
            }
            let color = if selected { JET } else { BONE };
            draw_text(&mut self.canvas, &self.fonts.label, &item.label, color, px(204.0), py(y + 24.0))?;
            draw_text(&mut self.canvas, &self.fonts.detail,
                if yes { if reboot { "Reboot Zaman" } else { "Shut down Zaman" } } else { "Return to the menu" },
                color, px(450.0), py(y + 45.0))?;
            if selected {
                draw_text(&mut self.canvas, &self.fonts.detail, ">", JET, px(1698.0), py(y + 45.0))?;
            }
        }
        if self.pending() {
            draw_text(&mut self.canvas, &self.fonts.detail,
                if reboot { "Rebooting..." } else { "Shutting down..." }, WARNING, px(156.0), py(831.0))?;
        } else if self.local_error.is_some() || !self.context.5.is_empty() {
            draw_text(&mut self.canvas, &self.fonts.detail,
                "Action could not complete. Select No to return to the menu.", WARNING, px(156.0), py(831.0))?;
        }
        draw_text(&mut self.canvas, &self.fonts.small,
            "UP/DOWN Choose   A Select   B Cancel   GUIDE Close menu", BONE, px(156.0), py(975.0))?;
        self.canvas.present();
        Ok(())
    }
    fn render(&mut self) -> Result<()> {
        let (width, height) = self.canvas.output_size().map_err(other)?;
        let scale = (width as f32 / DESIGN_WIDTH).min(height as f32 / DESIGN_HEIGHT);
        if (scale - self.font_scale).abs() > f32::EPSILON {
            self.fonts = LoadedFonts::load(self.ttf, &self.font_paths, scale)?;
            self.font_scale = scale;
        }
        if let Some(action) = self.model.power_action() {
            return self.render_power_confirmation(action, width, height);
        }
        let offset_x = (width as f32 - DESIGN_WIDTH * scale) / 2.0;
        let offset_y = (height as f32 - DESIGN_HEIGHT * scale) / 2.0;
        let px = |v: f32| (offset_x + v * scale).round() as i32;
        let py = |v: f32| (offset_y + v * scale).round() as i32;
        let pw = |v: f32| (v * scale).round() as u32;
        let ph = |v: f32| ((v * scale).round() as u32).max(1);

        let title = &self.fonts.title;
        let label = &self.fonts.label;
        let small = &self.fonts.small;
        let detail = &self.fonts.detail;

        self.canvas.set_draw_color(JET);
        self.canvas.clear();

        self.canvas.set_draw_color(OBSIDIAN);
        self.canvas
            .fill_rect(Rect::new(px(0.0), py(0.0), pw(1191.0), ph(DESIGN_HEIGHT)))
            .map_err(other)?;

        self.canvas.set_draw_color(SAND);
        self.canvas
            .fill_rect(Rect::new(px(0.0), py(0.0), pw(9.0), ph(DESIGN_HEIGHT)))
            .map_err(other)?;

        draw_text(&mut self.canvas, &title, "ZAMAN", BONE, px(96.0), py(63.0))?;
        draw_text(
            &mut self.canvas,
            &small,
            "SYSTEM MENU",
            SAND,
            px(100.0),
            py(204.0),
        )?;

        self.canvas.set_draw_color(CARAMEL);
        self.canvas
            .fill_rect(Rect::new(px(99.0), py(285.0), pw(975.0), ph(2.0)))
            .map_err(other)?;

        // Keep the password field immediately above the keyboard so each
        // controller press has visible feedback in the same area of the UI.
        let rows = if self.model.password_page() {
            RowLayout {
                top: 425.0,
                height: 90.0,
                gap: 12.0,
            }
        } else {
            // Mute remains a distinct focus target, rendered inside Volume.
            let count = self.model.items.len() - usize::from(self.model.page == MenuPage::Volume);
            RowLayout::new(count)
        };
        if self.model.password_page() {
            draw_text(
                &mut self.canvas,
                small,
                if self.model.page == MenuPage::BluetoothPin { "BLUETOOTH PIN / CODE" } else { "WI-FI PASSWORD" },
                SAND,
                px(100.0),
                py(300.0),
            )?;
            draw_text(
                &mut self.canvas,
                small,
                &format!("{}/{}", self.model.password.len(), self.model.password_limit()),
                SAND,
                px(992.0),
                py(300.0),
            )?;
            self.canvas.set_draw_color(CARAMEL);
            self.canvas
                .draw_rect(Rect::new(px(96.0), py(338.0), pw(978.0), ph(79.0)))
                .map_err(other)?;
            let mut entry = if self.model.password_visible {
                self.model.password.clone()
            } else {
                "*".repeat(self.model.password.len())
            };
            if entry.is_empty() {
                entry = if self.model.page == MenuPage::BluetoothPin { "Enter PIN / code" } else { "Enter password" }.into();
            } else {
                entry.push('|');
                let mut clipped = false;
                while label.size_of(&entry).map_err(other)?.0 > pw(900.0) {
                    entry.remove(0);
                    clipped = true;
                }
                if clipped {
                    while label.size_of(&format!("...{entry}")).map_err(other)?.0 > pw(900.0) {
                        entry.remove(0);
                    }
                    entry.insert_str(0, "...");
                }
            }
            draw_text(
                &mut self.canvas,
                label,
                &entry,
                if self.model.password.is_empty() { SAND } else { BONE },
                px(132.0),
                py(341.0),
            )?;
        }
        for (index, item) in self.model.items.iter().enumerate() {
            if self.model.page == MenuPage::Volume && item.command == MenuCommand::Mute {
                continue;
            }
            let visual_index = if self.model.page == MenuPage::Volume && index > 2 { index - 1 } else { index };
            let y = py(rows.y(visual_index));
            let selected = index == self.model.selected;
            self.canvas
                .set_draw_color(if selected { SAND } else { JET });
            self.canvas
                .fill_rect(Rect::new(px(96.0), y, pw(978.0), ph(rows.height)))
                .map_err(other)?;
            if !selected {
                self.canvas.set_draw_color(CARAMEL);
                self.canvas
                    .draw_rect(Rect::new(px(96.0), y, pw(978.0), ph(rows.height)))
                    .map_err(other)?;
            }
            let color = if selected { JET } else { BONE };
            if self.model.password_page() {
                let controls = [
                    "Space",
                    "Erase",
                    "Mode",
                    if self.model.password_visible {
                        "Hide"
                    } else {
                        "Show"
                    },
                    if self.model.page == MenuPage::BluetoothPin { "Confirm" } else { "Connect" },
                ];
                let keys: Vec<String> = if index < 4 {
                    self.model
                        .password_keys(index)
                        .iter()
                        .map(|byte| (*byte as char).to_string())
                        .collect()
                } else {
                    controls.iter().map(|text| (*text).to_string()).collect()
                };
                for (column, key) in keys.iter().enumerate() {
                    let (start, stride, cell_width) = if index < 4 {
                        (133.0, 90.0, 78.0)
                    } else {
                        (133.0, 180.0, 160.0)
                    };
                    let x = px(start + column as f32 * stride);
                    let width = pw(cell_width);
                    let focused = selected && column == self.model.password_col;
                    if focused {
                        self.canvas.set_draw_color(JET);
                        self.canvas
                            .fill_rect(Rect::new(
                                x,
                                y + ph(10.0) as i32,
                                width,
                                ph(rows.height - 20.0),
                            ))
                            .map_err(other)?;
                    }
                    let font = if index < 4 { label } else { detail };
                    let text_width = font.size_of(key).map_err(other)?.0 as i32;
                    draw_text(
                        &mut self.canvas,
                        font,
                        key,
                        if focused { SAND } else { color },
                        x + (width as i32 - text_width) / 2,
                        y + ph((rows.height - if index < 4 { 66.0 } else { 36.0 }) / 2.0) as i32,
                    )?;
                }
            } else {
                let mut visible = item.label.clone();
                let root = self.model.page == MenuPage::Root;
                if root && matches!(item.command, MenuCommand::System | MenuCommand::Reboot | MenuCommand::Shutdown) {
                    draw_cell_icon(&mut self.canvas, Rect::new(px(135.0), y + ph((rows.height - 64.0) / 2.0) as i32,
                        pw(64.0), ph(64.0)), item.command, color)?;
                }
                let label_width = if matches!(item.command, MenuCommand::AccessPoint(_) | MenuCommand::BluetoothDevice(_)) { 570.0 }
                    else if root { 736.0 } else { 830.0 };
                while !visible.is_empty() && label.size_of(&visible).map_err(other)?.0 > pw(label_width) {
                    visible.pop();
                }
                draw_text(
                    &mut self.canvas,
                    label,
                    &visible,
                    color,
                    px(if root { 229.0 } else { 135.0 }),
                    y + ph(if root { (rows.height - 76.0) / 2.0 } else { 8.0 }) as i32,
                )?;
            }
            if let MenuCommand::AccessPoint(index) = item.command {
                if let Some(ap) = self.model.network.as_ref().and_then(|n| n.access_points.get(index)) {
                    // A dark badge keeps both status colors readable on the
                    // amber selection highlight as well as the dark row.
                    self.canvas.set_draw_color(JET);
                    self.canvas.fill_rect(Rect::new(px(750.0),
                        y + ph((rows.height - 52.0) / 2.0) as i32, pw(270.0), ph(52.0)))
                        .map_err(other)?;
                    let (status, color) = if self.local_error.as_deref() == Some(NETWORK_STATUS_FAILED) {
                        ("Checking...", SAND)
                    } else if ap.active {
                        ("Connected", CONNECTED)
                    } else {
                        ("Disconnected", DISCONNECTED)
                    };
                    draw_text(&mut self.canvas, small, status, color, px(766.0),
                        y + ph((rows.height - 32.0) / 2.0) as i32)?;
                }
            }
            if let MenuCommand::BluetoothDevice(index) = item.command {
                if let Some(device) = self.model.bluetooth.as_ref().and_then(|state| state.devices.get(index)) {
                    self.canvas.set_draw_color(JET);
                    self.canvas.fill_rect(Rect::new(px(750.0),
                        y + ph((rows.height - 52.0) / 2.0) as i32, pw(270.0), ph(52.0))).map_err(other)?;
                    let (status, color) = if self.bluetooth_error.is_some() { ("Unavailable", SAND) }
                        else if device.connected { ("Connected", CONNECTED) } else { ("Disconnected", DISCONNECTED) };
                    draw_text(&mut self.canvas, small, status, color, px(766.0),
                        y + ph((rows.height - 32.0) / 2.0) as i32)?;
                }
            }
            if item.command == MenuCommand::AudioLevel {
                let level = self
                    .model
                    .audio
                    .map(AudioState::displayed_percent)
                    .unwrap_or(0);
                self.canvas.set_draw_color(CARAMEL);
                self.canvas
                    .fill_rect(Rect::new(
                        px(520.0),
                        y + ph((rows.height - 10.0) / 2.0) as i32,
                        pw(250.0),
                        ph(10.0),
                    ))
                    .map_err(other)?;
                self.canvas
                    .set_draw_color(if selected { JET } else { SAND });
                if level > 0 {
                    self.canvas
                        .fill_rect(Rect::new(
                            px(520.0),
                            y + ph((rows.height - 10.0) / 2.0) as i32,
                            pw(2.5 * level as f32),
                            ph(10.0),
                        ))
                        .map_err(other)?;
                }
                let volume = self
                    .model
                    .audio
                    .map(|_| format!("{level}%"))
                    .unwrap_or_else(|| "--".into());
                draw_text(
                    &mut self.canvas,
                    detail,
                    &volume,
                    color,
                    px(795.0),
                    y + ph((rows.height - 36.0) / 2.0) as i32,
                )?;
                let button = Rect::new(px(924.0), y + ph((rows.height - 82.0) / 2.0) as i32, pw(112.0), ph(82.0));
                let button_focused = self.model.items.get(self.model.selected)
                    .is_some_and(|item| item.command == MenuCommand::Mute);
                let muted = self.model.audio.is_some_and(|audio| audio.muted);
                self.canvas.set_draw_color(if button_focused { SAND } else { JET });
                self.canvas.fill_rect(button).map_err(other)?;
                self.canvas.set_draw_color(if button_focused { BONE } else { CARAMEL });
                self.canvas.draw_rect(button).map_err(other)?;
                let icon_color = if self.model.audio.is_none() { CARAMEL }
                    else if button_focused { JET }
                    else if muted { DISCONNECTED } else { BONE };
                draw_audio_icon(&mut self.canvas, button, muted, icon_color)?;
            }
            if selected && item.command != MenuCommand::AudioLevel {
                draw_text(
                    &mut self.canvas,
                    detail,
                    ">",
                    JET,
                    px(1032.0),
                    y + ((rows.height - 36.0) * 0.5 * scale).round() as i32,
                )?;
            }
        }

        if self.model.page == MenuPage::Volume {
            draw_text(&mut self.canvas, small, "CURRENT OUTPUT", SAND, px(1278.0), py(262.0))?;
            let current = self.model.audio_outputs.iter()
                .find(|output| Some(&output.name) == self.model.audio_default.as_ref());
            draw_wrapped(&mut self.canvas, detail,
                current.map(|output| output.description.as_str()).unwrap_or("Unavailable"),
                BONE, px(1278.0), py(297.0), pw(525.0), ph(108.0))?;
        }
        let state_label = if self.model.page == MenuPage::Volume {
            match self.model.audio {
                Some(audio) if self.model.audio_editing && audio.muted => "ADJUSTING VOLUME / MUTED",
                Some(_) if self.model.audio_editing => "ADJUSTING VOLUME",
                Some(audio) if audio.muted => "AUDIO MUTED",
                Some(_) => "AUDIO ON",
                None => "AUDIO UNAVAILABLE",
            }
        } else if self.model.bluetooth_page() {
            if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) { "PAIRING" } else { "BLUETOOTH" }
        } else if self.model.page == MenuPage::Display {
            if self.model.display.state.as_ref().is_some_and(|s| s.preview.is_some()) {
                "KEEP THIS DISPLAY MODE?"
            } else { "DISPLAY SETTINGS" }
        } else { match self.context.3.as_str() {
            "paused" => "GAME PAUSED",
            "running" => "GAME RUNNING",
            "unknown" => "RECOVERY NEEDED",
            _ => "GAME LIBRARY",
        }};
        let selected_item = self.model.items.get(self.model.selected);
        let state_detail = match selected_item {
            Some(MenuItem { command: MenuCommand::AudioOutput, .. }) => {
                let selected: String = self.model.selected_audio_output()
                    .map(|output| output.description.as_str()).unwrap_or("Unavailable")
                    .chars().take(44).collect();
                format!("Selected: {selected}\nLeft / Right chooses.\nApply output to use it.")
            }
            Some(MenuItem { command: MenuCommand::AudioApply, .. })
                if self.model.audio_selected == self.model.audio_default && self.model.audio_default.is_some() => {
                    "This output is already in use.".into()
                }
            Some(item) => item.detail.clone(),
            None => "Choose an action.".to_string(),
        };
        let state_detail = if self.model.page == MenuPage::Display {
            if self.model.display.state.as_ref().is_some_and(|s| s.preview.is_some()) {
                self.model.display.detail()
            } else { format!("{}\n\n{}", self.model.display.detail(), state_detail) }
        } else if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) {
            let instructions = self.model.bluetooth_prompt.as_ref().map(PairPrompt::detail)
                .unwrap_or_else(|| "Waiting for the other device. Put it in pairing mode. B cancels.".into());
            if self.model.page == MenuPage::BluetoothPin {
                let ready = self.model.bluetooth_prompt.as_ref().is_some_and(|prompt| prompt.valid_entry(&self.model.password));
                format!("{instructions}\n{}\nX / Mode changes keys.", if ready { "Ready to confirm." } else { "PIN / code appears above the keys." })
            } else { instructions }
        } else if self.model.password_page() {
            let ssid: String = self
                .model
                .pending_ssid
                .as_deref()
                .unwrap_or("Wi-Fi")
                .chars()
                .filter(|c| !c.is_control())
                .take(24)
                .collect();
            let status = if self.model.password.len() < 8 {
                "Enter at least 8 characters."
            } else {
                "Ready to connect."
            };
            format!("{ssid}\nPassword appears above the keys.\n{status}\nX / Mode: abc / ABC / symbols.")
        } else {
            state_detail
        };
        let network_page = matches!(self.model.page, MenuPage::Network | MenuPage::AccessPoints);
        if network_page {
            let network = self.model.network.as_ref().filter(|_| {
                self.local_error.as_deref() != Some(NETWORK_STATUS_FAILED)
            });
            let status = |connected: Option<bool>| match connected {
                Some(true) => ("Connected", CONNECTED),
                Some(false) => ("Disconnected", DISCONNECTED),
                None => ("Checking...", SAND),
            };
            let ethernet = status(network.map(|n| n.ethernet_connection.is_some()));
            let wifi = status(network.map(|n| n.wifi_enabled && n.wifi_connection.is_some()));
            for (index, (name, value, color)) in [
                ("ETH", ethernet.0, ethernet.1),
                ("", network.and_then(|n| n.ethernet_connection.as_deref()).unwrap_or("--"), BONE),
                ("WI-FI", wifi.0, wifi.1),
                ("", network.and_then(|n| n.wifi_ssid.as_deref().or(n.wifi_connection.as_deref())).unwrap_or("--"), BONE),
                ("IP", network.and_then(|n| n.address.as_deref()).unwrap_or("--"), BONE),
            ]
            .iter()
            .enumerate()
            {
                let limited: String = value.chars().filter(|c| !c.is_control()).take(25).collect();
                let text = if name.is_empty() { limited } else { format!("{name}: {limited}") };
                draw_text(
                    &mut self.canvas,
                    small,
                    &text,
                    *color,
                    px(1278.0),
                    py(262.0 + index as f32 * 35.0),
                )?;
            }
        }
        let bluetooth_overview = self.model.bluetooth_page()
            && !matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin);
        if bluetooth_overview {
            let adapter = self.model.bluetooth.as_ref().and_then(|state| state.adapter.as_ref());
            let selected_device = if self.model.page == MenuPage::BluetoothDevices {
                selected_item.and_then(|item| match item.command {
                    MenuCommand::BluetoothDevice(index) => self.model.bluetooth.as_ref()?.devices.get(index), _ => None,
                })
            } else { self.model.bluetooth_device.as_ref().filter(|_| matches!(self.model.page, MenuPage::BluetoothDevice | MenuPage::BluetoothForget)) };
            let (status, status_color) = if self.bluetooth_error.is_some() { ("Unavailable", SAND) }
                else if adapter.is_some_and(|adapter| adapter.powered) { ("On", CONNECTED) }
                else if adapter.is_some() { ("Off", DISCONNECTED) } else { ("No adapter", SAND) };
            let lines = if let Some(device) = selected_device {
                vec![(device.name.clone(), BONE), (device.address.clone(), BONE),
                    (if self.bluetooth_error.is_some() { "Status unavailable".into() }
                        else { format!("{} / {}", if device.paired { "Paired" } else { "Not paired" }, if device.connected { "Connected" } else { "Disconnected" }) },
                        if self.bluetooth_error.is_some() { SAND } else if device.connected { CONNECTED } else { DISCONNECTED }),
                    (device.battery.map(|level| format!("Battery: {level}%")).unwrap_or_else(|| "Battery: not reported".into()), BONE)]
            } else {
                let state = self.model.bluetooth.as_ref();
                vec![(adapter.map(|adapter| adapter.name.clone()).unwrap_or_else(|| "Checking adapter...".into()), BONE),
                    (format!("Bluetooth: {status}"), status_color),
                    (format!("{} devices / {} paired", state.map(|state| state.devices.len()).unwrap_or(0), state.map(|state| state.devices.iter().filter(|device| device.paired).count()).unwrap_or(0)), BONE),
                    (if adapter.is_some_and(|adapter| adapter.discovering) { "Discovery active".into() } else { "Discovery stopped".into() }, SAND)]
            };
            for (index, (text, color)) in lines.iter().enumerate() {
                let limited: String = text.chars().filter(|c| !c.is_control()).take(28).collect();
                draw_text(&mut self.canvas, small, &limited, *color, px(1278.0), py(262.0 + index as f32 * 35.0))?;
            }
        }
        draw_text(
            &mut self.canvas,
            &small,
            state_label,
            SAND,
            px(1278.0),
            py(if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) { 262.0 } else if self.model.page == MenuPage::Display { 300.0 } else if network_page || bluetooth_overview { 477.0 } else if self.model.page == MenuPage::Volume { 426.0 } else { 393.0 }),
        )?;
        draw_wrapped(
            &mut self.canvas,
            &detail,
            &state_detail,
            BONE,
            px(1278.0),
            py(if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) { 325.0 } else if self.model.page == MenuPage::Display { 363.0 } else if network_page || bluetooth_overview { 540.0 } else if self.model.page == MenuPage::Volume { 489.0 } else { 456.0 }),
            pw(525.0),
            ph(if matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) { 324.0 } else if self.model.page == MenuPage::Display { 276.0 } else if network_page || bluetooth_overview { 108.0 } else if self.model.page == MenuPage::Volume { 156.0 } else { 168.0 }),
        )?;
        draw_text(
            &mut self.canvas,
            &small,
            "KAWN ELECTRO",
            CARAMEL,
            px(1278.0),
            py(831.0),
        )?;

        let page_error = if self.model.bluetooth_page() {
            self.bluetooth_error.as_deref()
        } else if self.model.page == MenuPage::Volume {
            self.audio_error.as_deref()
        } else {
            self.local_error.as_deref()
        };
        let error = page_error
            .filter(|_| !self.request_pending)
            .or_else(|| {
                if self.context.5.is_empty() {
                    None
                } else {
                    Some(self.context.5.as_str())
                }
            });
        if (self.pending() || self.model.network_busy || self.model.display.changing
            || (self.model.bluetooth_page() && self.model.bluetooth_changing && !matches!(self.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin))
            || (self.model.page == MenuPage::Volume && self.model.audio_busy)) && error.is_none() {
            draw_text(
                &mut self.canvas,
                &detail,
                "Applying selection...",
                SAND,
                px(1278.0),
                py(666.0),
            )?;
        } else if self.model.scan_busy && error.is_none() && self.model.page == MenuPage::AccessPoints {
            draw_text(&mut self.canvas, &detail, "Scanning...", SAND, px(1278.0), py(666.0))?;
        } else if self.model.bluetooth_page() && self.model.bluetooth.as_ref().is_some_and(|state| state.own_scan) && error.is_none() {
            draw_text(&mut self.canvas, detail, "Finding devices...", SAND, px(1278.0), py(666.0))?;
        } else if error.is_some() {
            // Detailed service diagnostics remain in status/journal; keep the
            // on-screen recovery instruction short and readable at TV distance.
            draw_wrapped(
                &mut self.canvas,
                &detail,
                if self.model.bluetooth_page() && self.bluetooth_error.is_some() {
                    "Bluetooth action failed. Check the device, adapter and Bluetooth service, then try again."
                } else if self.model.page == MenuPage::Volume && self.audio_error.is_some() {
                    "Audio change could not complete. Refresh the output and try again."
                } else if self.local_error.as_deref() == Some(SAVED_PROFILE_FAILED) {
                    SAVED_PROFILE_FAILED
                } else if self.local_error.as_deref() == Some(NETWORK_STATUS_FAILED) {
                    NETWORK_STATUS_FAILED
                } else if self.model.page == MenuPage::Display {
                    "Display change could not complete. Refresh the display status and try again."
                } else if self.model.page != MenuPage::Root && self.local_error.is_some() {
                    "Network action failed. Check the connection and try again."
                } else if self.context.4 {
                    "Transfer cleanup failed. Restart the session before retrying."
                } else if self.context.5.starts_with("Transfer") {
                    "Transfer could not open. Check its installation and try again."
                } else {
                    "Action could not complete. Please try again."
                },
                SAND,
                px(1278.0),
                py(666.0),
                pw(525.0),
                ph(132.0),
            )?;
        }

        self.canvas.set_draw_color(CARAMEL);
        self.canvas
            .fill_rect(Rect::new(px(96.0), py(936.0), pw(1728.0), ph(2.0)))
            .map_err(other)?;
        draw_text(
            &mut self.canvas,
            &small,
            if self.model.page == MenuPage::Root {
                "UP/DOWN Navigate   A Select   B/GUIDE Resume"
            } else if self.model.page == MenuPage::Volume {
                if self.model.items.get(self.model.selected).is_some_and(|i| i.command == MenuCommand::AudioLevel) {
                    if self.model.audio_editing { "LEFT/RIGHT Volume   A Confirm   B Done" }
                    else { "A Adjust volume   RIGHT Mute   UP/DOWN Navigate   B/GUIDE Back" }
                } else if self.model.items.get(self.model.selected).is_some_and(|i| i.command == MenuCommand::Mute) {
                    if self.model.audio.is_some_and(|audio| audio.muted) {
                        "LEFT Volume   A Unmute   UP/DOWN Navigate   B/GUIDE Back"
                    } else {
                        "LEFT Volume   A Mute   UP/DOWN Navigate   B/GUIDE Back"
                    }
                } else if self.model.items.get(self.model.selected).is_some_and(|i| i.command == MenuCommand::AudioOutput) {
                    "LEFT/RIGHT Output   A Next   B/GUIDE Back"
                } else { "UP/DOWN Navigate   A Select   B/GUIDE Back" }
            } else if self.model.password_page() {
                "D-PAD Choose Key   A Enter   X Mode   B/GUIDE Back"
            } else {
                "UP/DOWN Navigate   LEFT/RIGHT Adjust   A Select   B/GUIDE Back"
            },
            BONE,
            px(99.0),
            py(975.0),
        )?;

        self.canvas.present();
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--preview") {
        return preview(&args);
    }
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "wayland");
    }
    let fonts = Fonts::new()?;
    let sdl = sdl2::init().map_err(other)?;
    let video = sdl.video().map_err(other)?;
    let ttf: &'static Sdl2TtfContext = Box::leak(Box::new(sdl2::ttf::init().map_err(other)?));
    let mut pump = sdl.event_pump().map_err(other)?;
    let connection = Builder::session()?.name(MENU_SERVICE)?.build().await?;
    let proxy = Proxy::new(&connection, SERVICE, PATH, INTERFACE).await?;
    let mut opened = proxy.receive_signal("MenuOpened").await?;
    let mut closed = proxy.receive_signal("MenuClosed").await?;
    let mut changed = proxy.receive_signal("MenuContextChanged").await?;
    let mut input = proxy.receive_signal("MenuInput").await?;
    let mut surface: Option<MenuSurface> = None;
    synchronize(&proxy, &video, ttf, &fonts, &mut surface).await?;
    let (results, mut result_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u64, std::result::Result<(), String>)>();
    let (audio_results, mut audio_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u64, std::result::Result<AudioSnapshot, String>)>();
    let (network_results, mut network_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u64, u64, NetworkRequest, std::result::Result<NetworkState, String>)>();
    let bluetooth_client = BluetoothClient::default();
    let (bluetooth_events, mut bluetooth_rx) = tokio::sync::mpsc::unbounded_channel::<BluetoothEvent>();
    let mut bluetooth_scope = None;
    let (display_results, mut display_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u64, std::result::Result<DisplayState, String>)>();
    if let Some(current) = surface.as_mut() {
        current.model.audio_busy = true;
        start_audio(
            AudioAction::Read,
            current.model.generation,
            audio_results.clone(),
        );
    }
    let mut tick = interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut status_tick = interval_at(Instant::now() + Duration::from_secs(3), Duration::from_secs(3));
    status_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut audio_tick = interval_at(Instant::now() + Duration::from_secs(3), Duration::from_secs(3));
    audio_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut display_tick = interval_at(Instant::now() + Duration::from_secs(1), Duration::from_secs(1));
    display_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut bluetooth_tick = interval_at(Instant::now() + Duration::from_secs(3), Duration::from_secs(3));
    bluetooth_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    println!("zaman-menu ready; embedded Saira Condensed / IBM Plex Mono");
    loop {
        let mut effect = ModelEffect::None;
        tokio::select! {
            signal=opened.next()=>{ signal.ok_or_else(||other("MenuOpened stream closed"))?; synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?; if let Some(current)=surface.as_mut().filter(|s| !s.model.audio_busy) { current.model.audio_busy=true; start_audio(AudioAction::Read,current.model.generation,audio_results.clone()); } }
            signal=closed.next()=>{ signal.ok_or_else(||other("MenuClosed stream closed"))?; synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?; }
            signal=changed.next()=>{ signal.ok_or_else(||other("MenuContextChanged stream closed"))?; synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?; }
            signal=input.next()=>{
                let signal=signal.ok_or_else(||other("MenuInput stream closed"))?;
                let (generation,event,value):(u64,String,f64)=signal.body().deserialize()?;
                if let Some(current)=surface.as_mut().filter(|s|s.model.generation==generation) { effect=current.input(&event,value); }
            }
            Some((generation,result))=result_rx.recv()=>{
                if let Some(current)=surface.as_mut().filter(|s|s.model.generation==generation) {
                    current.request_pending=false;
                    if let Err(error)=result { eprintln!("Menu action failed: {error}");current.local_error=Some(error); }
                }
                synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?;
            }
            Some((generation,result))=audio_rx.recv()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation) {
                    current.model.audio_busy=false;
                    match result {
                        Ok(snapshot)=>{current.model.update_audio(snapshot);current.audio_error=None;},
                        Err(error)=>{ eprintln!("Audio action failed: {error}");current.model.audio=None;current.model.audio_editing=false;current.audio_error=Some(error);current.model.rebuild(); }
                    }
                    current.render()?;
                }
            }
            _=audio_tick.tick()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.page==MenuPage::Volume && !s.model.audio_busy && !s.pending()) {
                    current.model.audio_busy=true;
                    start_audio(AudioAction::Read,current.model.generation,audio_results.clone());
                }
            }
            Some(event)=bluetooth_rx.recv()=>{
                match event {
                    BluetoothEvent::State { generation, operation, result } => {
                        if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation && s.model.bluetooth_operation==operation) {
                            current.model.bluetooth_busy=false;
                            current.model.bluetooth_changing=false;
                            current.clear_bluetooth_prompt();
                            match result {
                                Ok(state)=>{ current.model.update_bluetooth(state); current.bluetooth_error=None; }
                                Err(error)=>{
                                    if !current.model.bluetooth_cancelled {
                                        eprintln!("Bluetooth action failed: {error}"); current.bluetooth_error=Some(error);
                                    }
                                }
                            }
                            if matches!(current.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) {
                                current.model.set_page(if current.model.bluetooth_device.is_some() { MenuPage::BluetoothDevice } else { MenuPage::BluetoothDevices });
                            } else { current.model.rebuild(); }
                            current.model.bluetooth_cancelled=false;
                            current.render()?;
                        }
                    }
                    BluetoothEvent::Prompt { generation, operation, prompt, reply } => {
                        if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation && s.model.bluetooth_operation==operation
                            && s.model.bluetooth_busy && s.model.bluetooth_page() && !s.model.bluetooth_cancelled) {
                            current.clear_bluetooth_prompt();
                            let entry=prompt.entry();
                            current.model.bluetooth_prompt=Some(prompt);
                            current.bluetooth_reply=reply;
                            current.model.set_page(if entry { MenuPage::BluetoothPin } else { MenuPage::BluetoothPair });
                            current.render()?;
                        } else if let Some(reply)=reply { let _=reply.send(None); }
                    }
                    BluetoothEvent::Dismiss { generation, operation } => {
                        if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation && s.model.bluetooth_operation==operation) {
                            current.clear_bluetooth_prompt();
                            if matches!(current.model.page, MenuPage::BluetoothPair | MenuPage::BluetoothPin) { current.model.set_page(MenuPage::BluetoothPair); }
                            current.render()?;
                        }
                    }
                }
            }
            _=bluetooth_tick.tick()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.bluetooth_page() && !s.model.bluetooth_busy && !s.pending()) {
                    begin_bluetooth(current, &bluetooth_client, BluetoothAction::Read, bluetooth_events.clone());
                }
            }
            Some((generation,result))=display_rx.recv()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation) {
                    current.model.display.busy=false;
                    current.model.display.changing=false;
                    match result {
                        Ok(state)=>{
                            current.local_error=if state.error.is_empty() { None } else { Some(state.error.clone()) };
                            if let Some(selected)=current.model.display.update(state) {
                                current.model.selected=selected;
                                current.model.pressed.clear();
                            }
                            current.model.rebuild();
                        }
                        Err(error)=>{ eprintln!("Display action failed: {error}"); current.local_error=Some(error); }
                    }
                    current.render()?;
                }
            }
            _=display_tick.tick()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.page==MenuPage::Display && !s.model.display.busy && !s.pending()) {
                    current.model.display.busy=true;
                    start_display(connection.clone(),DisplayAction::Read,current.model.generation,display_results.clone());
                }
            }
            Some((generation,revision,request,result))=network_rx.recv()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation) {
                    let valid = revision == current.model.network_revision;
                    let mut redraw = false;
                    let saved_ssid = if let NetworkRequest::Saved(ssid) = &request { Some(ssid.clone()) } else { None };
                    match request {
                        NetworkRequest::Status => {
                            current.model.status_busy = false;
                            if valid {
                                match result {
                                    Ok(state) => {
                                        redraw = current.model.update_network(state, false);
                                        if current.local_error.as_deref() == Some(NETWORK_STATUS_FAILED) {
                                            current.local_error = None;
                                            redraw = true;
                                        }
                                    }
                                    Err(_) if current.local_error.is_none() => {
                                        current.local_error = Some(NETWORK_STATUS_FAILED.into());
                                        redraw = true;
                                    }
                                    Err(_) => {}
                                }
                            }
                        }
                        NetworkRequest::Scan => {
                            current.model.scan_busy = false;
                            if valid {
                                match result {
                                    Ok(state) => {
                                        current.model.update_network(state, true);
                                        current.local_error = None;
                                    }
                                    Err(error) => { eprintln!("Network scan failed: {error}"); current.local_error = Some(error); }
                                }
                                redraw = true;
                            }
                            current.model.rebuild();
                        }
                        NetworkRequest::Change | NetworkRequest::Saved(_) => {
                            current.model.network_busy = false;
                            if valid {
                                match result {
                                    Ok(state) => {
                                        current.model.update_network(state, false);
                                        current.model.retry_password = None;
                                        current.local_error = None;
                                        if current.model.page == MenuPage::Password {
                                            current.model.pending_ssid = None;
                                            current.model.password_visible = false;
                                            current.model.set_page(MenuPage::AccessPoints);
                                        }
                                        if current.model.page == MenuPage::AccessPoints { current.model.rescan_needed = true; }
                                    }
                                    Err(error) => {
                                        eprintln!("Network action failed: {error}");
                                        if let Some(ssid) = saved_ssid {
                                            current.model.retry_password = Some(ssid);
                                            current.local_error = Some(SAVED_PROFILE_FAILED.into());
                                        } else {
                                            current.local_error = Some(error);
                                        }
                                    }
                                }
                                redraw = true;
                            }
                        }
                    }
                    if current.model.rescan_needed && !current.model.scan_busy && !current.model.network_busy {
                        current.model.rescan_needed = false;
                        if current.model.page == MenuPage::AccessPoints {
                            current.model.scan_busy = true;
                            current.model.network_revision += 1;
                            start_network(NetworkAction::Read { scan: true }, generation, current.model.network_revision, NetworkRequest::Scan, network_results.clone());
                            redraw = true;
                        }
                    }
                    if redraw { current.render()?; }
                }
            }
            _=status_tick.tick()=>{
                if let Some(current)=surface.as_mut().filter(|s| matches!(s.model.page, MenuPage::Network | MenuPage::AccessPoints) && !s.model.status_busy && !s.model.network_busy && !s.model.scan_busy) {
                    current.model.status_busy=true;
                    start_network(NetworkAction::Read { scan: false }, current.model.generation, current.model.network_revision, NetworkRequest::Status, network_results.clone());
                }
            }
            _=tick.tick()=>{
                for event in pump.poll_iter() {
                    match event {
                        Event::Quit{..}=>{ bluetooth_client.close(); return Ok(()); },
                        Event::KeyDown{keycode:Some(key),repeat:false,..}=>{
                            if let Some(current)=surface.as_mut().filter(|s| !s.pending()) {
                                effect=current.model.keyboard_input(key);
                                if effect!=ModelEffect::None { break; }
                            }
                        }
                        Event::Window{win_event:sdl2::event::WindowEvent::Exposed
                            | sdl2::event::WindowEvent::Resized(_, _)
                            | sdl2::event::WindowEvent::SizeChanged(_, _),..}=>effect=ModelEffect::Redraw,
                        _=>{}
                    }
                }
            }
        }
        if let Some(current) = surface.as_mut() {
            match effect {
                ModelEffect::Bluetooth(action) => {
                    if !current.model.bluetooth_busy { begin_bluetooth(current, &bluetooth_client, action, bluetooth_events.clone()); }
                    current.render()?;
                }
                ModelEffect::BluetoothReply(value) => {
                    if let Some(reply)=current.bluetooth_reply.take() { let _=reply.send(value); }
                    current.clear_bluetooth_prompt();
                    current.model.set_page(MenuPage::BluetoothPair);
                    current.render()?;
                }
                ModelEffect::BluetoothCancel => {
                    current.clear_bluetooth_prompt();
                    bluetooth_client.cancel_pairing();
                    current.model.bluetooth_cancelled=true;
                    current.model.set_page(if current.model.bluetooth_device.is_some() { MenuPage::BluetoothDevice } else { MenuPage::BluetoothDevices });
                    current.render()?;
                }
                ModelEffect::Display(action) if !current.model.display.busy => {
                    current.model.display.busy=true;
                    current.model.display.changing=action!=DisplayAction::Read;
                    current.local_error=None;
                    current.model.pressed.clear();
                    start_display(connection.clone(),action,current.model.generation,display_results.clone());
                    current.render()?;
                }
                ModelEffect::Audio(action) if !current.model.audio_busy => {
                    current.model.audio_busy = true;
                    current.audio_error = None;
                    start_audio(action, current.model.generation, audio_results.clone());
                    current.render()?;
                }
                ModelEffect::Network(action) => {
                    match action {
                        NetworkAction::Read { scan: true } if current.model.scan_busy => {
                            current.model.rescan_needed = true;
                        }
                        NetworkAction::Read { scan: true } => {
                            current.model.scan_busy = true;
                            current.model.network_revision += 1;
                            current.local_error = None;
                            current.model.rebuild();
                            start_network(NetworkAction::Read { scan: true }, current.model.generation, current.model.network_revision, NetworkRequest::Scan, network_results.clone());
                            current.render()?;
                        }
                        NetworkAction::Read { scan: false } if !current.model.status_busy && !current.model.network_busy && !current.model.scan_busy => {
                            current.model.status_busy = true;
                            start_network(NetworkAction::Read { scan: false }, current.model.generation, current.model.network_revision, NetworkRequest::Status, network_results.clone());
                        }
                        NetworkAction::Read { scan: false } => {}
                        NetworkAction::Saved { ssid, uuid, device } if !current.model.network_busy => {
                            current.model.network_busy = true;
                            current.model.network_revision += 1;
                            if current.model.scan_busy && current.model.page == MenuPage::AccessPoints { current.model.rescan_needed = true; }
                            current.local_error = None;
                            start_network(NetworkAction::Saved { ssid: ssid.clone(), uuid, device }, current.model.generation, current.model.network_revision, NetworkRequest::Saved(ssid), network_results.clone());
                            current.render()?;
                        }
                        action if !current.model.network_busy => {
                            current.model.network_busy = true;
                            current.model.network_revision += 1;
                            if current.model.scan_busy && current.model.page == MenuPage::AccessPoints { current.model.rescan_needed = true; }
                            current.local_error = None;
                            start_network(action, current.model.generation, current.model.network_revision, NetworkRequest::Change, network_results.clone());
                            current.render()?;
                        }
                        _ => {}
                    }
                }
                ModelEffect::Activate(command) if !current.pending() => {
                    current.request_pending = true;
                    current.local_error = None;
                    current.model.pressed.clear();
                    current.render()?;
                    println!("Activating {}.", command.method());
                    let generation = current.model.generation;
                    let connection = connection.clone();
                    let results = results.clone();
                    tokio::spawn(async move {
                        let result = request_action(&connection, generation, command)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = results.send((generation, result));
                    });
                }
                ModelEffect::Redraw => {
                    if current.model.page == MenuPage::Password && current.local_error.as_deref() == Some(SAVED_PROFILE_FAILED) {
                        current.local_error = None;
                    }
                    current.render()?;
                }
                _ => {}
            }
        }
        let next_scope=surface.as_ref().filter(|current| current.model.bluetooth_page()).map(|current| current.model.generation);
        if bluetooth_scope.is_some() && bluetooth_scope!=next_scope {
            if let Some(current)=surface.as_mut() { current.clear_bluetooth_prompt(); }
            bluetooth_client.close();
        }
        bluetooth_scope=next_scope;
    }
}

fn begin_bluetooth(current: &mut MenuSurface, client: &BluetoothClient, action: BluetoothAction,
    events: tokio::sync::mpsc::UnboundedSender<BluetoothEvent>) {
    current.model.bluetooth_busy=true;
    current.model.bluetooth_changing=action!=BluetoothAction::Read;
    current.model.bluetooth_operation+=1;
    current.model.bluetooth_cancelled=false;
    if action!=BluetoothAction::Read { current.bluetooth_error=None; }
    client.start(action, current.model.generation, current.model.bluetooth_operation, events);
}

fn start_display(
    connection: Connection,
    action: DisplayAction,
    generation: u64,
    sender: tokio::sync::mpsc::UnboundedSender<(u64, std::result::Result<DisplayState, String>)>,
) {
    tokio::spawn(async move {
        let result = async {
            let proxy = Proxy::new(&connection, SERVICE, PATH, INTERFACE).await?;
            let json: String = match action {
                DisplayAction::Read => proxy.call("DisplayStatus", &()).await?,
                DisplayAction::Preview { output, mode } => proxy.call("PreviewDisplay",
                    &(generation, output, mode.width, mode.height, mode.refresh)).await?,
                DisplayAction::Confirm(token) => proxy.call("ConfirmDisplay", &(generation, token)).await?,
                DisplayAction::Revert(token) => proxy.call("RevertDisplay", &(generation, token)).await?,
            };
            Ok::<DisplayState, Box<dyn Error + Send + Sync>>(serde_json::from_str(&json)?)
        }.await.map_err(|e| e.to_string());
        let _ = sender.send((generation, result));
    });
}

fn start_audio(
    action: AudioAction,
    generation: u64,
    sender: tokio::sync::mpsc::UnboundedSender<(u64, std::result::Result<AudioSnapshot, String>)>,
) {
    tokio::task::spawn_blocking(move || {
        let result = audio::apply(action).map_err(|error| error.to_string());
        let _ = sender.send((generation, result));
    });
}

fn start_network(
    action: NetworkAction,
    generation: u64,
    revision: u64,
    request: NetworkRequest,
    sender: tokio::sync::mpsc::UnboundedSender<(u64, u64, NetworkRequest, std::result::Result<NetworkState, String>)>,
) {
    tokio::task::spawn_blocking(move || {
        let result = network::apply(action).map_err(|error| error.to_string());
        let _ = sender.send((generation, revision, request, result));
    });
}

async fn synchronize(
    proxy: &Proxy<'_>,
    video: &sdl2::VideoSubsystem,
    ttf: &'static Sdl2TtfContext,
    paths: &Fonts,
    surface: &mut Option<MenuSurface>,
) -> Result<()> {
    let context: MenuContextTuple = proxy.call("MenuContext", &()).await?;
    if !context.0 {
        *surface = None;
        return Ok(());
    }
    let generation = context.1;
    let new = surface
        .as_ref()
        .is_none_or(|s| s.model.generation != generation);
    if new {
        println!(
            "Opening menu generation={generation} return_target={}",
            context.2
        );
        *surface = Some(MenuSurface::open(video, ttf, paths, context)?);
    } else if let Some(current) = surface.as_mut() {
        current.update(context);
    }
    surface.as_mut().unwrap().render()?;
    if new {
        // A close can race the initial context read. Reconcile next notification
        // rather than terminating the persistent menu on a stale acknowledgement.
        if let Err(error) = proxy
            .call::<_, _, ()>("MenuPresented", &(generation,))
            .await
        {
            eprintln!("Presentation acknowledgement was superseded: {error}");
        }
    }
    Ok(())
}
async fn request_action(
    connection: &Connection,
    generation: u64,
    command: MenuCommand,
) -> Result<()> {
    let proxy = Proxy::new(connection, SERVICE, PATH, INTERFACE).await?;
    let _: () = proxy
        .call("RequestMenuAction", &(generation, command.action()))
        .await?;
    Ok(())
}

fn draw_text(
    canvas: &mut Canvas<Window>,
    font: &Font<'_, '_>,
    text: &str,
    color: Color,
    x: i32,
    y: i32,
) -> Result<()> {
    let surface = font.render(text).blended(color).map_err(other)?;
    let creator = canvas.texture_creator();
    let texture = creator
        .create_texture_from_surface(&surface)
        .map_err(other)?;
    let q = texture.query();
    canvas
        .copy(&texture, None, Rect::new(x, y, q.width, q.height))
        .map_err(other)?;
    Ok(())
}
// Native-scale line icons avoid relying on font symbol coverage.
fn draw_cell_icon(canvas: &mut Canvas<Window>, bounds: Rect, command: MenuCommand, color: Color) -> Result<()> {
    let scale = bounds.width().min(bounds.height()) as f32 / 64.0;
    let cx = bounds.x() as f32 + bounds.width() as f32 / 2.0;
    let cy = bounds.y() as f32 + bounds.height() as f32 / 2.0;
    let point = |x: f32, y: f32| ((cx + x * scale).round() as i32, (cy + y * scale).round() as i32);
    let stroke = (2.5 * scale).round().max(1.0) as i32;
    canvas.set_draw_color(color);
    let line = |canvas: &mut Canvas<Window>, a: (f32, f32), b: (f32, f32)| -> Result<()> {
        let a = point(a.0, a.1);
        let b = point(b.0, b.1);
        for offset in 0..stroke {
            canvas.draw_line((a.0 + offset, a.1), (b.0 + offset, b.1)).map_err(other)?;
        }
        Ok(())
    };
    let arc = |canvas: &mut Canvas<Window>, radius: f32, start: f32, end: f32| -> Result<()> {
        for step in 0..48 {
            let a = (start + (end - start) * step as f32 / 48.0).to_radians();
            let b = (start + (end - start) * (step + 1) as f32 / 48.0).to_radians();
            line(canvas, (radius * a.cos(), radius * a.sin()), (radius * b.cos(), radius * b.sin()))?;
        }
        Ok(())
    };
    match command {
        MenuCommand::System => {
            for step in 0..32 {
                let vertex = |i: usize| {
                    let angle = (i as f32 * 360.0 / 32.0).to_radians();
                    let radius = if matches!(i % 4, 1 | 2) { 25.0 } else { 19.0 };
                    (radius * angle.cos(), radius * angle.sin())
                };
                line(canvas, vertex(step), vertex((step + 1) % 32))?;
            }
            arc(canvas, 8.0, 0.0, 360.0)?;
        }
        MenuCommand::Reboot => {
            arc(canvas, 22.0, 20.0, 315.0)?;
            line(canvas, (13.4, -27.6), (15.6, -15.6))?;
            line(canvas, (3.5, -17.7), (15.6, -15.6))?;
        }
        MenuCommand::Shutdown => {
            arc(canvas, 22.0, -55.0, 235.0)?;
            line(canvas, (0.0, -27.0), (0.0, -4.0))?;
        }
        _ => {}
    }
    Ok(())
}

// Draw the speaker and mute/wave marks geometrically: embedded fonts need
// no extra symbol glyphs, and the icon follows the same output scale as the UI.
fn draw_audio_icon(canvas: &mut Canvas<Window>, button: Rect, muted: bool, color: Color) -> Result<()> {
    let scale = button.height() as f32 / 82.0;
    let cx = button.x() as f32 + button.width() as f32 / 2.0 - 5.0 * scale;
    let cy = button.y() as f32 + button.height() as f32 / 2.0;
    let point = |x: f32, y: f32| ((cx + x * scale).round() as i32, (cy + y * scale).round() as i32);
    canvas.set_draw_color(color);
    let strip_height = scale.ceil().max(1.0) as u32;
    for y in -22i32..=22 {
        let left = if y.abs() <= 9 { -24.0 } else { (y.abs() - 9) as f32 - 12.0 };
        let (x, top) = point(left, y as f32);
        canvas.fill_rect(Rect::new(x, top, ((4.0 - left) * scale).ceil().max(1.0) as u32, strip_height)).map_err(other)?;
    }
    let stroke = (2.0 * scale).round().max(1.0) as i32;
    let line = |canvas: &mut Canvas<Window>, start: (f32, f32), end: (f32, f32)| -> Result<()> {
        let start = point(start.0, start.1);
        let end = point(end.0, end.1);
        for offset in 0..stroke {
            canvas.draw_line((start.0 + offset, start.1), (end.0 + offset, end.1)).map_err(other)?;
        }
        Ok(())
    };
    if muted {
        line(canvas, (13.0, -12.0), (34.0, 12.0))?;
        line(canvas, (13.0, 12.0), (34.0, -12.0))?;
    } else {
        for radius in [15.0f32, 25.0] {
            for step in 0..10 {
                let a = (-60.0 + step as f32 * 12.0).to_radians();
                let b = (-60.0 + (step + 1) as f32 * 12.0).to_radians();
                line(canvas, (4.0 + radius * a.cos(), radius * a.sin()),
                    (4.0 + radius * b.cos(), radius * b.sin()))?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_wrapped(
    canvas: &mut Canvas<Window>,
    font: &Font<'_, '_>,
    text: &str,
    color: Color,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<()> {
    let surface = font
        .render(text)
        .blended_wrapped(color, width)
        .map_err(other)?;
    let creator = canvas.texture_creator();
    let texture = creator
        .create_texture_from_surface(&surface)
        .map_err(other)?;
    let q = texture.query();
    let h = q.height.min(height);
    canvas
        .copy(
            &texture,
            Some(Rect::new(0, 0, q.width, h)),
            Rect::new(x, y, q.width, h),
        )
        .map_err(other)?;
    Ok(())
}
fn other(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

// Deterministic rendering of the production renderer only. No session or
// controller access, and not a graphical/hardware acceptance test.
fn preview(args: &[String]) -> Result<()> {
    if args.len() != 6 {
        return Err(other("usage: zaman-menu --preview library|game|system|display|display-preview|network|networks|bluetooth|bluetooth-devices|bluetooth-pair|bluetooth-pin|password|password-typed|password-shown|volume|volume-edit|muted|mute-icon|audio-output|exit|reboot|shutdown|reboot-confirm|reboot-confirm-yes|reboot-confirm-library|shutdown-confirm|shutdown-confirm-yes|shutdown-confirm-library|error|pending WIDTH HEIGHT /absolute/output.bmp STATE").into());
    }
    let width: u32 = args[2].parse()?;
    let height: u32 = args[3].parse()?;
    let path = Path::new(&args[4]);
    if !path.is_absolute() {
        return Err(other("preview path must be absolute").into());
    }
    if !(320..=4096).contains(&width) || !(240..=2160).contains(&height) {
        return Err(other("unsupported preview dimensions").into());
    }
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    let sdl = sdl2::init().map_err(other)?;
    let video = sdl.video().map_err(other)?;
    let ttf: &'static Sdl2TtfContext = Box::leak(Box::new(sdl2::ttf::init().map_err(other)?));
    let window = video
        .window("Zaman renderer preview", width, height)
        .hidden()
        .build()
        .map_err(other)?;
    let canvas = window.into_canvas().software().build().map_err(other)?;
    let game = !matches!(args[1].as_str(), "library" | "reboot-confirm-library" | "shutdown-confirm-library");
    let context = (
        true,
        1,
        if game { "game" } else { "library" }.into(),
        args[5].clone(),
        args[1] == "pending",
        if args[1] == "error" {
            "Preview error"
        } else {
            ""
        }
        .into(),
        if game {
            vec![
                "resume".into(),
                "exit-game".into(),
                "reboot".into(),
                "shutdown".into(),
            ]
        } else {
            vec![
                "resume".into(),
                "transfer".into(),
                "reboot".into(),
                "shutdown".into(),
            ]
        },
    );
    let mut model = MenuModel::new(&context);
    match args[1].as_str() {
        "display" | "display-preview" => {
            model.set_page(MenuPage::Display);
            if let Some(selected) = model.display.update(display::preview_state(args[1] == "display-preview")) {
                model.selected = selected;
            }
            model.rebuild();
        }
        "system" => model.set_page(MenuPage::System),
        "volume" | "volume-edit" | "muted" | "mute-icon" | "audio-output" => model.set_page(MenuPage::Volume),
        "bluetooth" | "bluetooth-devices" | "bluetooth-pair" | "bluetooth-pin" => {
            model.set_page(if args[1] == "bluetooth" { MenuPage::Bluetooth } else { MenuPage::BluetoothDevices });
            let controller = bluetooth::Device { path: "/org/bluez/hci0/dev_A".into(), adapter: "/org/bluez/hci0".into(), address: "AA:00:00:00:00:01".into(), name: "Game controller".into(), paired: true, connected: true, battery: Some(80), ..Default::default() };
            model.update_bluetooth(BluetoothState { adapter: Some(bluetooth::Adapter { path: "/org/bluez/hci0".into(), name: "Zaman Bluetooth".into(), powered: true, ..Default::default() }),
                devices: vec![controller.clone(), bluetooth::Device { path: "/org/bluez/hci0/dev_B".into(), address: "BB:00:00:00:00:02".into(), name: "Headphones".into(), paired: false, connected: false, battery: None, ..controller }], own_scan: false });
            if matches!(args[1].as_str(), "bluetooth-pair" | "bluetooth-pin") {
                model.bluetooth_prompt = Some(PairPrompt { token: 1, device: "Headphones".into(), kind: if args[1] == "bluetooth-pin" { PromptKind::Passkey } else { PromptKind::Confirm(123) } });
                model.set_page(if args[1] == "bluetooth-pin" { MenuPage::BluetoothPin } else { MenuPage::BluetoothPair });
            }
        }
        "network" => model.set_page(MenuPage::Network),
        "networks" => model.set_page(MenuPage::AccessPoints),
        "password" | "password-typed" | "password-shown" => {
            model.pending_ssid = Some("Home Network".into());
            model.set_page(MenuPage::Password);
            if args[1] == "password-typed" || args[1] == "password-shown" {
                model.password = "example8".into();
            }
            model.password_visible = args[1] == "password-shown";
        }
        "exit" => model.selected = 1,
        "reboot" => model.selected = 2,
        "shutdown" => model.selected = 3,
        "reboot-confirm" | "reboot-confirm-yes" | "reboot-confirm-library" => {
            model.set_page(MenuPage::RebootConfirm);
            if args[1] == "reboot-confirm-yes" { model.selected = 0; }
        }
        "shutdown-confirm" | "shutdown-confirm-yes" | "shutdown-confirm-library" => {
            model.set_page(MenuPage::ShutdownConfirm);
            if args[1] == "shutdown-confirm-yes" { model.selected = 0; }
        }
        _ => {}
    }
    if matches!(
        model.page,
        MenuPage::Network | MenuPage::AccessPoints | MenuPage::Password
    ) {
        model.network = Some(NetworkState {
            wifi_enabled: true,
            wifi_device: Some("wlan0".into()),
            wifi_connection: Some("Home Network".into()),
            wifi_ssid: Some("Home Network".into()),
            ethernet_connection: None,
            address: Some("192.168.1.24/24".into()),
            access_points: vec![
                network::AccessPoint {
                    ssid: "Home Network".into(),
                    signal: 86,
                    security: "WPA2".into(),
                    active: true,
                },
                network::AccessPoint {
                    ssid: "Guest".into(),
                    signal: 62,
                    security: "--".into(),
                    active: false,
                },
            ],
            saved_profiles: Default::default(),
        });
        model.rebuild();
    }
    let (out_w, out_h) = canvas.output_size().map_err(other)?;
    let scale = (out_w as f32 / DESIGN_WIDTH).min(out_h as f32 / DESIGN_HEIGHT);
    let paths = Fonts::new()?;
    let fonts = LoadedFonts::load(ttf, &paths, scale)?;
    let mut menu = MenuSurface {
        canvas,
        model,
        context,
        request_pending: false,
        local_error: None,
        audio_error: None,
        bluetooth_error: None,
        bluetooth_reply: None,
        fonts,
        font_paths: paths,
        ttf,
        font_scale: scale,
    };
    menu.model.update_audio(AudioSnapshot {
        level: Some(AudioState { percent: 65, muted: args[1] == "muted" }),
        outputs: vec![
            AudioOutput { id: 42, serial: "100".into(), name: "hdmi".into(), description: "HDMI / Acer XB273U V3".into() },
            AudioOutput { id: 43, serial: "101".into(), name: "usb".into(), description: "USB headphones".into() },
        ],
        default_name: Some("hdmi".into()),
    });
    if matches!(args[1].as_str(), "volume" | "muted") { menu.model.selected = 1; }
    if args[1] == "volume-edit" { menu.model.selected = 1; menu.model.audio_editing = true; menu.model.rebuild(); }
    if args[1] == "mute-icon" { menu.model.selected = 2; }
    if args[1] == "audio-output" {
        menu.model.audio_selected = Some("usb".into());
        menu.model.selected = 0;
        menu.model.rebuild();
    }
    menu.render()?;
    let mut pixels = menu
        .canvas
        .read_pixels(None, PixelFormatEnum::RGB24)
        .map_err(other)?;
    let surface = sdl2::surface::Surface::from_data(
        &mut pixels,
        width,
        height,
        width * 3,
        PixelFormatEnum::RGB24,
    )
    .map_err(other)?;
    surface.save_bmp(path).map_err(other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(game: bool) -> MenuContextTuple {
        (
            true,
            7,
            if game { "game" } else { "library" }.into(),
            "paused".into(),
            false,
            String::new(),
            if game {
                vec!["resume", "exit-game", "reboot", "shutdown"]
            } else {
                vec!["resume", "transfer", "reboot", "shutdown"]
            }
            .into_iter()
            .map(str::to_string)
            .collect(),
        )
    }

    #[test]
    fn system_navigation_volume_and_existing_actions_are_reachable() {
        for game in [false, true] {
            let mut model = MenuModel::new(&context(game));
            assert_eq!(model.items[0].command, MenuCommand::System);
            if !game {
                assert_eq!(model.items[1].command, MenuCommand::Transfer);
            }
            assert_eq!(model.items.last().unwrap().command, MenuCommand::Shutdown);
            let initial = AudioState { percent: 50, muted: false };
            model.audio = Some(initial);
            model.selected = 0;
            assert_eq!(model.selected_command(), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::System);
            assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::None);
            assert_eq!(model.selected_command(), ModelEffect::Audio(AudioAction::Read));
            assert_eq!(model.page, MenuPage::Volume);
            model.selected = 1;
            assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
            assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::Redraw);
            assert!(model.audio_editing);
            assert_eq!(model.normalized_input("ui_right", 1.0), ModelEffect::Audio(AudioAction::Set(55)));
            assert_eq!(model.normalized_input("ui_right", 1.0), ModelEffect::None);
            model.normalized_input("ui_right", 0.0);
            assert_eq!(model.normalized_input("ui_left", 1.0), ModelEffect::Audio(AudioAction::Set(45)));
            assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
            assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::System);
            assert_eq!(model.audio, Some(initial));
            assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::None);

            assert_eq!(model.selected_command(), ModelEffect::Audio(AudioAction::Read));
            assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Redraw);
            assert_eq!(model.items[model.selected].command, MenuCommand::Mute);
            assert_eq!(model.items[2].label, "Mute");
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Audio(AudioAction::SetMute(true)));
            model.audio = Some(AudioState { percent: 50, muted: true });
            model.rebuild();
            assert_eq!(model.items[2].label, "Unmute");
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Audio(AudioAction::SetMute(false)));
            assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::Redraw);
            assert_eq!(model.items[model.selected].command, MenuCommand::AudioLevel);
            assert_eq!(model.audio.unwrap().percent, 50);
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Redraw);
            assert!(model.audio_editing);
            assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::Audio(AudioAction::Set(45)));
            assert_eq!(model.audio.unwrap().displayed_percent(), 50);
            assert!(model.audio.unwrap().muted);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
            assert!(!model.audio_editing);
            assert_eq!(model.page, MenuPage::Volume);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::System);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::Root);
            model.selected = model.items.len() - 1;
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::ShutdownConfirm);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Activate(MenuCommand::Close));
        }
    }

    #[test]
    fn root_has_no_resume_cell_and_preserves_exit_before_power_actions() {
        for game in [false, true] {
            let model = MenuModel::new(&context(game));
            assert_eq!(model.items.iter().map(|item| item.command).collect::<Vec<_>>(),
                vec![MenuCommand::System, if game { MenuCommand::ExitGame } else { MenuCommand::Transfer },
                    MenuCommand::Reboot, MenuCommand::Shutdown]);
        }
    }

    #[test]
    fn power_requires_a_separate_yes_and_cancel_restores_focus() {
        for game in [false, true] {
            for action in [MenuCommand::Reboot, MenuCommand::Shutdown] {
                let mut model = MenuModel::new(&context(game));
                let origin = model.items.iter().position(|item| item.command == action).unwrap();
                model.selected = origin;
                assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
                assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::Redraw);
                assert_eq!(model.power_action(), Some(action));
                assert_eq!(model.items[model.selected].command, MenuCommand::PowerNo);
                // The opening click's repeated release cannot confirm anything.
                assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::None);
                assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Redraw);
                assert_eq!(model.page, MenuPage::Root);
                assert_eq!(model.selected, origin);
                model.keyboard_input(Keycode::Return);
                model.keyboard_input(Keycode::Up);
                assert_eq!(model.items[model.selected].command, MenuCommand::PowerYes);
                assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
                assert_eq!(model.page, MenuPage::Root);
                assert_eq!(model.selected, origin);
                model.keyboard_input(Keycode::Return);
                model.keyboard_input(Keycode::Up);
                assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
                assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::Activate(action));
            }
        }
    }

    #[test]
    fn power_confirmation_respects_suspension_and_revoked_actions() {
        for action in [MenuCommand::Reboot, MenuCommand::Shutdown] {
            let mut ctx = context(true);
            let mut model = MenuModel::new(&ctx);
            model.selected = model.items.iter().position(|item| item.command == action).unwrap();
            model.keyboard_input(Keycode::Return);
            model.keyboard_input(Keycode::Up);
            assert_eq!(model.items[model.selected].command, MenuCommand::PowerYes);
            ctx.4 = true;
            model.update(&ctx);
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::None);
            assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
            ctx.4 = false;
            ctx.6.retain(|name| name != action.action());
            model.update(&ctx);
            assert_eq!(model.page, MenuPage::Root);
            assert!(model.power_action().is_none());
            assert!(!model.items.iter().any(|item| item.command == action));
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Activate(MenuCommand::Close));
        }
    }

    #[test]
    fn volume_horizontal_focus_and_vertical_rows_match_the_layout() {
        let mut model = MenuModel::new(&context(false));
        model.set_page(MenuPage::Volume);
        model.update_audio(audio_snapshot("hdmi", 50, false));
        assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Redraw);
        assert_eq!(model.items[model.selected].command, MenuCommand::Mute);
        assert_eq!(model.audio.unwrap().percent, 50);
        assert_eq!(model.keyboard_input(Keycode::Down), ModelEffect::Redraw);
        assert_eq!(model.items[model.selected].command, MenuCommand::AudioApply);
        model.keyboard_input(Keycode::Up);
        assert_eq!(model.items[model.selected].command, MenuCommand::AudioLevel);
        model.keyboard_input(Keycode::Down);
        assert_eq!(model.items[model.selected].command, MenuCommand::AudioApply);
        model.keyboard_input(Keycode::Up);
        model.keyboard_input(Keycode::Right);
        model.keyboard_input(Keycode::Up);
        assert_eq!(model.items[model.selected].command, MenuCommand::AudioOutput);
        model.keyboard_input(Keycode::Down);
        model.keyboard_input(Keycode::Return);
        assert!(model.audio_editing);
        assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Audio(AudioAction::Set(55)));
        // A readback for the same output must not end an adjustment.
        model.update_audio(audio_snapshot("hdmi", 55, false));
        assert!(model.audio_editing);
        model.update_audio(audio_snapshot("usb", 25, true));
        assert!(!model.audio_editing);
        model.keyboard_input(Keycode::Return);
        assert!(model.audio_editing);
        model.keyboard_input(Keycode::Down);
        assert!(!model.audio_editing);
        assert_eq!(model.items[model.selected].command, MenuCommand::AudioApply);
    }

    #[test]
    fn volume_waits_for_audio_but_back_remains_available() {
        let mut model = MenuModel::new(&context(false));
        model.set_page(MenuPage::Volume);
        for audio in [None, Some(AudioState { percent: 50, muted: false })] {
            model.audio = audio;
            model.audio_busy = audio.is_some();
            model.selected = 1;
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::None);
            assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Redraw);
            assert!(!model.audio_editing);
            model.selected = 2;
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::None);
        }
        assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::System);
    }

    fn audio_snapshot(default: &str, percent: u8, muted: bool) -> AudioSnapshot {
        AudioSnapshot {
            level: Some(AudioState { percent, muted }),
            outputs: vec![
                AudioOutput { id: 42, serial: "100".into(), name: "hdmi".into(), description: "HDMI monitor".into() },
                AudioOutput { id: 43, serial: "101".into(), name: "usb".into(), description: "USB headphones".into() },
            ],
            default_name: Some(default.into()),
        }
    }

    #[test]
    fn choosing_output_waits_for_apply_and_readback_preserves_candidate() {
        let mut model = MenuModel::new(&context(false));
        model.set_page(MenuPage::Volume);
        model.selected = 0;
        model.update_audio(audio_snapshot("hdmi", 55, false));
        assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Redraw);
        assert_eq!(model.audio_selected.as_deref(), Some("usb"));
        assert_eq!(model.audio_default.as_deref(), Some("hdmi"));
        model.update_audio(audio_snapshot("hdmi", 60, false));
        assert_eq!(model.audio_selected.as_deref(), Some("usb"));
        assert_eq!(model.audio.unwrap().percent, 60);
        model.selected = 3;
        let selected = model.selected_audio_output().unwrap().clone();
        assert_eq!(model.selected_command(), ModelEffect::Audio(AudioAction::SetDefault(selected)));
        model.update_audio(audio_snapshot("usb", 25, true));
        assert_eq!(model.audio_default.as_deref(), Some("usb"));
        assert_eq!(model.audio.unwrap(), AudioState { percent: 25, muted: true });
        assert_eq!(model.items[2].label, "Unmute");
        assert_eq!(model.selected_command(), ModelEffect::None);
    }

    #[test]
    fn removed_output_resets_candidate_and_busy_audio_does_not_apply() {
        let mut model = MenuModel::new(&context(false));
        model.set_page(MenuPage::Volume);
        model.selected = 0;
        model.update_audio(audio_snapshot("hdmi", 55, false));
        model.keyboard_input(Keycode::Right);
        let mut next = audio_snapshot("hdmi", 55, false);
        next.outputs.retain(|output| output.name != "usb");
        model.update_audio(next);
        assert_eq!(model.audio_selected.as_deref(), Some("hdmi"));
        model.update_audio(audio_snapshot("hdmi", 55, false));
        model.keyboard_input(Keycode::Right);
        model.audio_busy = true;
        assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::None);
        model.selected = 3;
        assert_eq!(model.selected_command(), ModelEffect::None);
        assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
        model.audio_busy = false;
        model.set_page(MenuPage::Volume);
        assert_eq!(model.audio_selected.as_deref(), Some("hdmi"));
    }

    #[test]
    fn transfer_suspension_blocks_audio_and_preserves_selection() {
        let mut model = MenuModel::new(&context(false));
        model.selected = 1;
        assert_eq!(
            model.keyboard_input(Keycode::Return),
            ModelEffect::Activate(MenuCommand::Transfer)
        );
        let mut next = context(false);
        next.4 = true;
        model.update(&next);
        assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::None);
        assert_eq!(model.normalized_input("ui_right", 1.0), ModelEffect::None);
        next.4 = false;
        model.update(&next);
        assert_eq!(model.selected, 1);
    }

    #[test]
    fn volume_is_clamped_and_rows_fit() {
        let mut model = MenuModel::new(&context(true));
        model.set_page(MenuPage::Volume);
        model.selected = 1;
        model.audio = Some(AudioState {
            percent: 99,
            muted: false,
        });
        assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::Redraw);
        assert!(model.audio_editing);
        assert_eq!(
            model.keyboard_input(Keycode::Right),
            ModelEffect::Audio(AudioAction::Set(100))
        );
        model.audio = Some(AudioState {
            percent: 0,
            muted: false,
        });
        assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::None);
        assert_eq!(
            parse_volume("Volume: 0.42 [MUTED]").unwrap(),
            AudioState {
                percent: 42,
                muted: true
            }
        );
        for count in [3, 4, 5] {
            let rows = RowLayout::new(count);
            assert!(rows.y(count - 1) + rows.height < 936.0);
        }
    }

    fn bluetooth_state() -> BluetoothState {
        BluetoothState {
            adapter: Some(bluetooth::Adapter { path: "/org/bluez/hci0".into(), name: "Zaman".into(), powered: true, discovering: false }),
            devices: vec![bluetooth::Device { path: "/org/bluez/hci0/dev_A".into(), adapter: "/org/bluez/hci0".into(), address: "AA:00:00:00:00:01".into(), name: "Controller".into(), ..Default::default() }],
            own_scan: false,
        }
    }

    #[test]
    fn bluetooth_pair_forget_and_pin_use_local_confirmation() {
        for game in [false, true] {
            let mut model = MenuModel::new(&context(game));
            model.set_page(MenuPage::System);
            model.selected = model.items.iter().position(|item| item.command == MenuCommand::Bluetooth).unwrap();
            assert_eq!(model.selected_command(), ModelEffect::Bluetooth(BluetoothAction::Read));
            model.update_bluetooth(bluetooth_state());
            model.selected = 2;
            assert!(matches!(model.selected_command(), ModelEffect::Bluetooth(BluetoothAction::Scan { start: true, .. })));
            assert_eq!(model.page, MenuPage::BluetoothDevices);
            assert_eq!(model.selected_command(), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::BluetoothDevice);
            assert!(matches!(model.selected_command(), ModelEffect::Bluetooth(BluetoothAction::Pair(_))));
            assert_eq!(model.page, MenuPage::BluetoothPair);
            assert_eq!(model.items[model.selected].command, MenuCommand::BluetoothCancel);
            assert_eq!(model.back(), ModelEffect::BluetoothCancel);
            model.bluetooth_prompt = Some(PairPrompt { token: 1, device: "Controller".into(), kind: PromptKind::Passkey });
            model.set_page(MenuPage::BluetoothPin);
            model.selected = 4; model.password_col = 4;
            model.password = "123".into();
            assert_eq!(model.selected_command(), ModelEffect::None);
            model.password = "000123".into();
            assert_eq!(model.selected_command(), ModelEffect::BluetoothReply(Some("000123".into())));
            model.set_page(MenuPage::BluetoothForget);
            assert_eq!(model.items[model.selected].command, MenuCommand::Back);
            assert_eq!(model.selected_command(), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::BluetoothDevice);
        }
    }

    #[test]
    fn bluetooth_refresh_keeps_identity_and_removed_selection_moves_to_back() {
        let mut model = MenuModel::new(&context(false));
        model.set_page(MenuPage::BluetoothDevices);
        let mut state = bluetooth_state();
        let controller = state.devices[0].clone();
        model.update_bluetooth(state.clone());
        state.devices.insert(0, bluetooth::Device { path: "/org/bluez/hci0/dev_B".into(), address: "BB:00:00:00:00:02".into(), name: "Headphones".into(), ..controller.clone() });
        model.update_bluetooth(state.clone());
        assert_eq!(model.items[model.selected].command, MenuCommand::BluetoothDevice(1));
        state.devices.retain(|device| device.address != controller.address);
        model.update_bluetooth(state.clone());
        assert_eq!(model.items[model.selected].command, MenuCommand::Back);
        model.bluetooth_device = Some(controller);
        model.set_page(MenuPage::BluetoothDevice);
        model.update_bluetooth(state);
        assert_eq!(model.page, MenuPage::BluetoothDevices);
        assert!(model.bluetooth_device.is_none());
    }

    #[test]
    fn network_pages_back_and_password_never_escape_to_daemon() {
        let mut model = MenuModel::new(&context(false));
        model.selected = 0;
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        model.selected = 1;
        assert!(matches!(
            model.selected_command(),
            ModelEffect::Network(NetworkAction::Read { scan: false })
        ));
        assert_eq!(model.page, MenuPage::Network);
        model.network = Some(NetworkState {
            wifi_enabled: true,
            wifi_device: Some("wlan0".into()),
            access_points: vec![network::AccessPoint {
                ssid: "Home".into(),
                signal: 75,
                security: "WPA2".into(),
                active: false,
            }],
            ..NetworkState::default()
        });
        model.selected = 1;
        assert!(matches!(
            model.selected_command(),
            ModelEffect::Network(NetworkAction::Read { scan: true })
        ));
        assert_eq!(model.page, MenuPage::AccessPoints);
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::Password);
        assert_eq!(model.keyboard_input(Keycode::Right), ModelEffect::Redraw);
        assert_eq!(model.password_col, 1);
        assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::Redraw);
        assert_eq!(model.password_col, 0);
        assert_eq!(model.keyboard_input(Keycode::Down), ModelEffect::Redraw);
        assert_eq!(model.selected, 1);
        assert_eq!(model.keyboard_input(Keycode::Up), ModelEffect::Redraw);
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert_eq!(model.password, "q");
        model.selected = 4;
        model.password_col = 4;
        assert_eq!(model.selected_command(), ModelEffect::None); // WPA passphrases need 8 characters
        model.password_col = 2;
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        model.password_col = 3;
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert!(model.password_visible);
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert!(!model.password_visible);
        model.selected = 0;
        model.password_col = 0;
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert_eq!(model.password, "qQ");
        model.selected = 4;
        model.password_col = 2;
        model.selected_command();
        model.selected = 0;
        model.password_col = 0;
        model.selected_command();
        assert_eq!(model.password, "qQ!");
        for _ in 0..5 {
            model.selected_command();
        }
        model.selected = 4;
        model.password_col = 4;
        assert!(matches!(
            model.selected_command(),
            ModelEffect::Network(NetworkAction::Connect {
                password: Some(_),
                ..
            })
        ));
        assert!(model.password.is_empty());
        assert_eq!(model.back(), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::AccessPoints);
        assert_eq!(model.back(), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::Network);
    }

    #[test]
    fn x_cycles_password_keyboard_only_once_per_press() {
        let mut model = MenuModel::new(&context(false));
        assert_eq!(model.normalized_input("ui_context", 1.0), ModelEffect::None);
        assert_eq!(model.normalized_input("ui_context", 0.0), ModelEffect::None);
        assert_eq!(model.keyboard_input(Keycode::X), ModelEffect::None);

        model.set_page(MenuPage::Password);
        for expected in [1, 2, 0] {
            assert_eq!(model.normalized_input("ui_context", 1.0), ModelEffect::None);
            assert_eq!(
                model.normalized_input("ui_context", 0.0),
                ModelEffect::Redraw
            );
            assert_eq!(model.password_layer, expected);
            assert_eq!(model.normalized_input("ui_context", 0.0), ModelEffect::None);
            assert_eq!(model.password_layer, expected);
        }
        assert_eq!(model.keyboard_input(Keycode::X), ModelEffect::Redraw);
        assert_eq!(model.password_layer, 1);
    }

    #[test]
    fn saved_wifi_reuses_profile_and_failed_profile_offers_password() {
        let mut model = MenuModel::new(&context(false));
        model.network = Some(NetworkState {
            wifi_device: Some("wlan0".into()),
            access_points: vec![network::AccessPoint {
                ssid: "Home".into(),
                signal: 80,
                security: "WPA2".into(),
                active: false,
            }],
            saved_profiles: [("Home".into(), "saved-uuid".into())].into(),
            ..NetworkState::default()
        });
        model.set_page(MenuPage::AccessPoints);
        assert_eq!(model.selected_command(), ModelEffect::Network(NetworkAction::Saved {
            ssid: "Home".into(), uuid: "saved-uuid".into(), device: "wlan0".into(),
        }));
        model.retry_password = Some("Home".into());
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::Password);
        assert!(!model.password_visible);
    }

    #[test]
    fn status_preserves_scan_and_scan_does_not_block_back() {
        let mut model = MenuModel::new(&context(false));
        model.network = Some(NetworkState {
            access_points: vec![network::AccessPoint {
                ssid: "Home".into(), signal: 80, security: "WPA2".into(), active: false,
            }],
            ..NetworkState::default()
        });
        model.set_page(MenuPage::AccessPoints);
        model.scan_busy = true;
        assert!(model.update_network(NetworkState { wifi_enabled: true, wifi_device: Some("wlan0".into()), ..NetworkState::default() }, false));
        assert_eq!(model.network.as_ref().unwrap().access_points[0].ssid, "Home");
        assert_eq!(model.normalized_input("ui_back", 1.0), ModelEffect::None);
        assert_eq!(model.normalized_input("ui_back", 0.0), ModelEffect::Redraw);
        assert_eq!(model.page, MenuPage::Network);
    }

    #[test]
    fn cached_network_badges_follow_ssid_changes_and_disconnects() {
        let mut model = MenuModel::new(&context(false));
        model.network = Some(NetworkState {
            wifi_enabled: true,
            wifi_device: Some("wlan0".into()),
            wifi_connection: Some("renamed profile".into()),
            wifi_ssid: Some("Home".into()),
            access_points: vec![
                network::AccessPoint { ssid: "Home".into(), signal: 80, security: "WPA2".into(), active: true },
                network::AccessPoint { ssid: "Guest".into(), signal: 60, security: "--".into(), active: false },
            ],
            ..NetworkState::default()
        });
        model.set_page(MenuPage::AccessPoints);
        model.selected = 1;
        assert!(model.update_network(NetworkState {
            wifi_enabled: true,
            wifi_device: Some("wlan0".into()),
            wifi_connection: Some("another profile name".into()),
            wifi_ssid: Some("Guest".into()),
            ..NetworkState::default()
        }, false));
        let rows = &model.network.as_ref().unwrap().access_points;
        assert!(!rows[0].active);
        assert!(rows[1].active);
        assert_eq!(rows[0].signal, 80);
        assert_eq!(model.items[model.selected].command, MenuCommand::AccessPoint(1));
        assert!(model.update_network(NetworkState {
            wifi_enabled: true, wifi_device: Some("wlan0".into()), ..NetworkState::default()
        }, false));
        assert!(model.network.as_ref().unwrap().access_points.iter().all(|ap| !ap.active));
        assert_eq!(model.network.as_ref().unwrap().access_points.len(), 2);
        assert!(model.update_network(NetworkState::default(), false));
        assert!(model.network.as_ref().unwrap().access_points.is_empty());
    }

    #[test]
    fn scan_reordering_preserves_the_selected_network() {
        let mut model = MenuModel::new(&context(false));
        let home = network::AccessPoint {
            ssid: "Home".into(),
            signal: 80,
            security: "WPA2".into(),
            active: false,
        };
        let guest = network::AccessPoint {
            ssid: "Guest".into(),
            signal: 60,
            security: "--".into(),
            active: false,
        };
        model.network = Some(NetworkState {
            access_points: vec![home.clone(), guest.clone()],
            ..NetworkState::default()
        });
        model.set_page(MenuPage::AccessPoints);
        model.selected = 1;
        assert!(model.update_network(NetworkState {
            access_points: vec![guest, home],
            ..NetworkState::default()
        }, true));
        assert_eq!(model.selected, 0);
        assert_eq!(model.items[model.selected].label, "Guest");
        model.selected = model.items.len() - 1;
        assert!(model.update_network(NetworkState {
            access_points: vec![network::AccessPoint {
                ssid: "Third".into(), signal: 40, security: "--".into(), active: false,
            }],
            ..NetworkState::default()
        }, true));
        assert_eq!(model.items[model.selected].command, MenuCommand::Back);
    }
}
