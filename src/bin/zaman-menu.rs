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
use std::process::Command;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use zaman_sessiond::contract::{MenuContextTuple, INTERFACE, MENU_SERVICE, PATH, SERVICE};
use zbus::{connection::Builder, Connection, Proxy};
#[path = "zaman-menu/network.rs"]
mod network;
use network::{NetworkAction, NetworkState};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const PRIMARY: &[u8] =
    include_bytes!("../../assets/fonts/saira-condensed/SairaCondensed-Regular.ttf");
const SECONDARY: &[u8] = include_bytes!("../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");
const JET: Color = Color::RGB(16, 12, 6);
const OBSIDIAN: Color = Color::RGB(27, 22, 16);
const SAND: Color = Color::RGB(230, 178, 90);
const BONE: Color = Color::RGB(242, 236, 224);
const CARAMEL: Color = Color::RGB(196, 117, 66);

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
    Volume,
    System,
    Network,
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
            "resume" => Some(item(
                "Resume",
                if context.2 == "game" {
                    "Return to current game"
                } else {
                    "Return to game library"
                },
                MenuCommand::Close,
            )),
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
                "Exit game and power off.",
                MenuCommand::Shutdown,
            )),
            _ => None,
        })
        .collect();
    if !result.is_empty() {
        result.insert(
            1,
            item("System", "Volume and network settings", MenuCommand::System),
        );
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AudioState {
    percent: u8,
    muted: bool,
}

impl AudioState {
    fn displayed_percent(self) -> u8 {
        if self.muted {
            0
        } else {
            self.percent
        }
    }
}

fn parse_volume(output: &str) -> Result<AudioState> {
    let value = output
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| other("missing volume"))?;
    let percent = (value.parse::<f32>()? * 100.0).round().clamp(0.0, 100.0) as u8;
    Ok(AudioState {
        percent,
        muted: output.contains("[MUTED]"),
    })
}

fn wpctl(args: &[&str]) -> Result<String> {
    let output = Command::new("wpctl").args(args).output()?;
    if !output.status.success() {
        return Err(other(format!(
            "wpctl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn read_audio() -> Result<AudioState> {
    parse_volume(&wpctl(&["get-volume", "@DEFAULT_AUDIO_SINK@"])?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AudioAction {
    Read,
    Set(u8),
    ToggleMute,
}

fn apply_audio(action: AudioAction) -> Result<AudioState> {
    match action {
        AudioAction::Read => {}
        AudioAction::Set(percent) => {
            wpctl(&[
                "set-volume",
                "@DEFAULT_AUDIO_SINK@",
                &format!("{}%", percent),
            ])?;
        }
        AudioAction::ToggleMute => {
            wpctl(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])?;
        }
    }
    read_audio()
}

#[derive(Debug, PartialEq)]
enum ModelEffect {
    None,
    Redraw,
    Activate(MenuCommand),
    Audio(AudioAction),
    Network(NetworkAction),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuPage {
    Root,
    System,
    Network,
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
    audio_busy: bool,
    pressed: BTreeSet<String>,
    items: Vec<MenuItem>,
    root_items: Vec<MenuItem>,
    page: MenuPage,
    network: Option<NetworkState>,
    network_busy: bool,
    ap_page: usize,
    pending_ssid: Option<String>,
    password: String,
    password_col: usize,
    password_layer: usize,
}
impl MenuModel {
    fn new(context: &MenuContextTuple) -> Self {
        Self {
            generation: context.1,
            selected: 0,
            suspended: context.4,
            audio: None,
            audio_busy: false,
            pressed: BTreeSet::new(),
            items: items(context),
            root_items: items(context),
            page: MenuPage::Root,
            network: None,
            network_busy: false,
            ap_page: 0,
            pending_ssid: None,
            password: String::new(),
            password_col: 0,
            password_layer: 0,
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
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }
    fn set_page(&mut self, page: MenuPage) {
        self.page = page;
        self.selected = 0;
        if page == MenuPage::Password {
            self.password_col = 0;
            self.password_layer = 0;
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
            4
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
            if self.password.len() < 63 {
                self.password
                    .push(self.password_keys(self.selected)[self.password_col] as char);
            }
            return ModelEffect::Redraw;
        }
        match self.password_col {
            0 => {
                if self.password.len() < 63 {
                    self.password.push(' ');
                }
                ModelEffect::Redraw
            }
            1 => {
                self.password.pop();
                ModelEffect::Redraw
            }
            2 => self.cycle_password_mode(),
            3 if !self.network_busy && (8..=63).contains(&self.password.len()) => {
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
            MenuPage::System => vec![
                item(
                    "Volume",
                    "Left / Right to adjust. Press A to mute.",
                    MenuCommand::Volume,
                ),
                item(
                    "Network",
                    "View connections and configure Wi-Fi.",
                    MenuCommand::Network,
                ),
                item("Back", "Return to the main menu.", MenuCommand::Back),
            ],
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
                                "{} / {}% signal{}",
                                if ap.security.is_empty() || ap.security == "--" {
                                    "Open"
                                } else {
                                    &ap.security
                                },
                                ap.signal,
                                if ap.active { " / Connected" } else { "" }
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
                        "No networks",
                        "Select Back, then Refresh to scan again.",
                        MenuCommand::Refresh,
                    ));
                }
                rows.push(item("Back", "Return to Network.", MenuCommand::Back));
                rows
            }
            MenuPage::Password => (0..5)
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
    fn selected_command(&mut self) -> ModelEffect {
        if self.page == MenuPage::Password {
            return self.password_accept();
        }
        let Some(command) = self.items.get(self.selected).map(|item| item.command) else {
            return ModelEffect::None;
        };
        match command {
            MenuCommand::Volume if !self.audio_busy && self.audio.is_some() => {
                ModelEffect::Audio(AudioAction::ToggleMute)
            }
            MenuCommand::Volume => ModelEffect::None,
            MenuCommand::System => {
                self.set_page(MenuPage::System);
                ModelEffect::Redraw
            }
            MenuCommand::Network => {
                self.set_page(MenuPage::Network);
                ModelEffect::Network(NetworkAction::Read { scan: false })
            }
            MenuCommand::Wifi if !self.network_busy => ModelEffect::Network(NetworkAction::Wifi(
                !self.network.as_ref().is_some_and(|n| n.wifi_enabled),
            )),
            MenuCommand::Networks if !self.network_busy => {
                self.ap_page = 0;
                self.set_page(MenuPage::AccessPoints);
                ModelEffect::Network(NetworkAction::Read { scan: true })
            }
            MenuCommand::Refresh if !self.network_busy => {
                ModelEffect::Network(NetworkAction::Read { scan: true })
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
        if self.page == MenuPage::AccessPoints
            && self
                .items
                .get(self.selected)
                .is_some_and(|item| item.command == MenuCommand::Page)
        {
            self.page_step(right);
            return ModelEffect::Redraw;
        }
        if self.page == MenuPage::Password {
            let count = self.password_width();
            self.password_col = if right {
                (self.password_col + 1) % count
            } else {
                (self.password_col + count - 1) % count
            };
            return ModelEffect::Redraw;
        }
        if self.audio_busy
            || self.items.get(self.selected).map(|item| item.command) != Some(MenuCommand::Volume)
        {
            return ModelEffect::None;
        }
        let Some(audio) = self.audio.filter(|audio| !audio.muted) else {
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
        self.selected = if up {
            (self.selected + self.items.len() - 1) % self.items.len()
        } else {
            (self.selected + 1) % self.items.len()
        };
        if self.page == MenuPage::Password {
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
                "ui_context" if self.page == MenuPage::Password => self.cycle_password_mode(),
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
            Keycode::X if self.page == MenuPage::Password => self.cycle_password_mode(),
            _ => ModelEffect::None,
        }
    }
}

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
    fonts: LoadedFonts,
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
            .position_centered()
            .borderless()
            .allow_highdpi()
            .fullscreen_desktop()
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
            fonts,
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
        if self.pending() || self.model.network_busy {
            if value <= 0.5 {
                self.model.pressed.remove(event);
            }
            return ModelEffect::None;
        }
        self.model.normalized_input(event, value)
    }
    fn render(&mut self) -> Result<()> {
        let (width, height) = self.canvas.output_size().map_err(other)?;
        let scale = (width as f32 / DESIGN_WIDTH).min(height as f32 / DESIGN_HEIGHT);
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

        let rows = RowLayout::new(self.model.items.len());
        for (index, item) in self.model.items.iter().enumerate() {
            let y = py(rows.y(index));
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
            if self.model.page == MenuPage::Password {
                let controls = ["Space", "Erase", "Mode", "Connect"];
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
                        (133.0, 225.0, 206.0)
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
                while !visible.is_empty() && label.size_of(&visible).map_err(other)?.0 > pw(830.0) {
                    visible.pop();
                }
                draw_text(
                    &mut self.canvas,
                    label,
                    &visible,
                    color,
                    px(135.0),
                    y + (8.0 * scale).round() as i32,
                )?;
            }
            if item.command == MenuCommand::Volume {
                let level = self
                    .model
                    .audio
                    .map(AudioState::displayed_percent)
                    .unwrap_or(0);
                self.canvas.set_draw_color(CARAMEL);
                self.canvas
                    .fill_rect(Rect::new(
                        px(693.0),
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
                            px(693.0),
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
                    px(958.0),
                    y + ph((rows.height - 36.0) / 2.0) as i32,
                )?;
            }
            if selected {
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

        let state_label = match self.context.3.as_str() {
            "paused" => "GAME PAUSED",
            "running" => "GAME RUNNING",
            "unknown" => "RECOVERY NEEDED",
            _ => "GAME LIBRARY",
        };
        let selected_item = self.model.items.get(self.model.selected);
        let state_detail = match selected_item {
            Some(MenuItem {
                command: MenuCommand::Volume,
                ..
            }) if self.model.audio.is_some_and(|audio| audio.muted) => {
                "Muted. Press A to restore the previous volume.".to_string()
            }
            Some(item) => item.detail.clone(),
            None => "Choose an action.".to_string(),
        };
        let state_detail = if self.model.page == MenuPage::Password {
            let ssid: String = self
                .model
                .pending_ssid
                .as_deref()
                .unwrap_or("Wi-Fi")
                .chars()
                .filter(|c| !c.is_control())
                .take(24)
                .collect();
            format!(
                "{}\nPassword: {} (min 8)\nD-pad picks; A enters.\nX / Mode: abc / ABC / symbols.",
                ssid,
                self.model.password.len()
            )
        } else {
            state_detail
        };
        if matches!(self.model.page, MenuPage::Network | MenuPage::AccessPoints) {
            let network = self.model.network.as_ref();
            for (index, (name, value)) in [
                (
                    "ETH",
                    network
                        .and_then(|n| n.ethernet_connection.as_deref())
                        .unwrap_or("Disconnected"),
                ),
                (
                    "WI-FI",
                    network
                        .and_then(|n| n.wifi_connection.as_deref())
                        .unwrap_or("Disconnected"),
                ),
                (
                    "IP",
                    network.and_then(|n| n.address.as_deref()).unwrap_or("--"),
                ),
            ]
            .iter()
            .enumerate()
            {
                let limited: String = value.chars().filter(|c| !c.is_control()).take(25).collect();
                draw_text(
                    &mut self.canvas,
                    small,
                    &format!("{name}: {limited}"),
                    BONE,
                    px(1278.0),
                    py(278.0 + index as f32 * 35.0),
                )?;
            }
        }
        draw_text(
            &mut self.canvas,
            &small,
            state_label,
            SAND,
            px(1278.0),
            py(393.0),
        )?;
        draw_wrapped(
            &mut self.canvas,
            &detail,
            &state_detail,
            BONE,
            px(1278.0),
            py(456.0),
            pw(525.0),
            ph(168.0),
        )?;
        draw_text(
            &mut self.canvas,
            &small,
            "KAWN ELECTRO",
            CARAMEL,
            px(1278.0),
            py(831.0),
        )?;

        let error = self
            .local_error
            .as_deref()
            .filter(|_| !self.request_pending)
            .or_else(|| {
                if self.context.5.is_empty() {
                    None
                } else {
                    Some(self.context.5.as_str())
                }
            });
        if (self.pending() || self.model.network_busy) && error.is_none() {
            draw_text(
                &mut self.canvas,
                &detail,
                "Applying selection...",
                SAND,
                px(1278.0),
                py(666.0),
            )?;
        } else if error.is_some() {
            // Detailed service diagnostics remain in status/journal; keep the
            // on-screen recovery instruction short and readable at TV distance.
            draw_wrapped(
                &mut self.canvas,
                &detail,
                if self.model.page != MenuPage::Root && self.local_error.is_some() {
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
            } else if self.model.page == MenuPage::Password {
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
        tokio::sync::mpsc::unbounded_channel::<(u64, std::result::Result<AudioState, String>)>();
    let (network_results, mut network_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u64, std::result::Result<NetworkState, String>)>();
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
    println!("zaman-menu ready; embedded Saira Condensed / IBM Plex Mono");
    loop {
        let mut effect = ModelEffect::None;
        tokio::select! {
            signal=opened.next()=>{ signal.ok_or_else(||other("MenuOpened stream closed"))?; synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?; if let Some(current)=surface.as_mut() { current.model.audio_busy=true; start_audio(AudioAction::Read,current.model.generation,audio_results.clone()); } }
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
                    match result { Ok(audio)=>{current.model.audio=Some(audio);current.local_error=None;}, Err(error)=>{ eprintln!("Audio action failed: {error}");current.local_error=Some(error); } }
                    current.render()?;
                }
            }
            Some((generation,result))=network_rx.recv()=>{
                if let Some(current)=surface.as_mut().filter(|s| s.model.generation==generation) {
                    current.model.network_busy=false;
                    match result {
                        Ok(state)=>{
                            current.model.network=Some(state);
                            current.model.ap_page=0;
                            current.local_error=None;
                            if current.model.page==MenuPage::Password {
                                current.model.pending_ssid=None;
                                current.model.set_page(MenuPage::AccessPoints);
                            } else { current.model.rebuild(); }
                        }
                        Err(error)=>{ eprintln!("Network action failed: {error}"); current.local_error=Some(error); }
                    }
                    current.render()?;
                }
            }
            _=tick.tick()=>{
                for event in pump.poll_iter() {
                    match event {
                        Event::Quit{..}=>return Ok(()),
                        Event::KeyDown{keycode:Some(key),repeat:false,..}=>{
                            if let Some(current)=surface.as_mut().filter(|s| !s.pending() && !s.model.network_busy) {
                                effect=current.model.keyboard_input(key);
                                if effect!=ModelEffect::None { break; }
                            }
                        }
                        Event::Window{win_event:sdl2::event::WindowEvent::Exposed,..}=>effect=ModelEffect::Redraw,
                        _=>{}
                    }
                }
            }
        }
        if let Some(current) = surface.as_mut() {
            match effect {
                ModelEffect::Audio(action) if !current.model.audio_busy => {
                    current.model.audio_busy = true;
                    start_audio(action, current.model.generation, audio_results.clone());
                }
                ModelEffect::Network(action) if !current.model.network_busy => {
                    current.model.network_busy = true;
                    current.local_error = None;
                    start_network(action, current.model.generation, network_results.clone());
                    current.render()?;
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
                    current.render()?;
                }
                _ => {}
            }
        }
    }
}

fn start_audio(
    action: AudioAction,
    generation: u64,
    sender: tokio::sync::mpsc::UnboundedSender<(u64, std::result::Result<AudioState, String>)>,
) {
    tokio::task::spawn_blocking(move || {
        let result = apply_audio(action).map_err(|error| error.to_string());
        let _ = sender.send((generation, result));
    });
}

fn start_network(
    action: NetworkAction,
    generation: u64,
    sender: tokio::sync::mpsc::UnboundedSender<(u64, std::result::Result<NetworkState, String>)>,
) {
    tokio::task::spawn_blocking(move || {
        let result = network::apply(action).map_err(|error| error.to_string());
        let _ = sender.send((generation, result));
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
        return Err(other("usage: zaman-menu --preview library|game|system|network|networks|password|volume|muted|exit|reboot|shutdown|error|pending WIDTH HEIGHT /absolute/output.bmp STATE").into());
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
    let game = args[1] != "library";
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
        "system" => model.set_page(MenuPage::System),
        "volume" | "muted" => model.set_page(MenuPage::System),
        "network" => model.set_page(MenuPage::Network),
        "networks" => model.set_page(MenuPage::AccessPoints),
        "password" => {
            model.pending_ssid = Some("Home Network".into());
            model.set_page(MenuPage::Password);
        }
        "exit" => model.selected = 2,
        "reboot" => model.selected = 3,
        "shutdown" => model.selected = 4,
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
        fonts,
    };
    menu.model.audio = Some(AudioState {
        percent: 65,
        muted: args[1] == "muted",
    });
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
            assert_eq!(model.items[1].command, MenuCommand::System);
            if !game {
                assert_eq!(model.items[2].command, MenuCommand::Transfer);
            }
            assert_eq!(model.items.last().unwrap().command, MenuCommand::Shutdown);
            model.audio = Some(AudioState {
                percent: 50,
                muted: false,
            });
            model.selected = 1;
            assert_eq!(model.selected_command(), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::System);
            assert_eq!(
                model.normalized_input("ui_right", 1.0),
                ModelEffect::Audio(AudioAction::Set(55))
            );
            assert_eq!(model.normalized_input("ui_right", 1.0), ModelEffect::None);
            model.normalized_input("ui_right", 0.0);
            assert_eq!(
                model.normalized_input("ui_left", 1.0),
                ModelEffect::Audio(AudioAction::Set(45))
            );
            assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
            assert_eq!(
                model.normalized_input("ui_accept", 0.0),
                ModelEffect::Audio(AudioAction::ToggleMute)
            );
            assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::None);
            model.audio_busy = true;
            assert_eq!(model.keyboard_input(Keycode::Return), ModelEffect::None);
            model.audio_busy = false;
            model.audio = Some(AudioState {
                percent: 50,
                muted: true,
            });
            assert_eq!(model.keyboard_input(Keycode::Left), ModelEffect::None);
            assert_eq!(
                model.keyboard_input(Keycode::Return),
                ModelEffect::Audio(AudioAction::ToggleMute)
            );
            assert_eq!(model.audio.unwrap().displayed_percent(), 0);
            assert_eq!(model.audio.unwrap().percent, 50);
            assert_eq!(model.keyboard_input(Keycode::Escape), ModelEffect::Redraw);
            assert_eq!(model.page, MenuPage::Root);
            model.selected = model.items.len() - 1;
            assert_eq!(
                model.keyboard_input(Keycode::Return),
                ModelEffect::Activate(MenuCommand::Shutdown)
            );
            assert_eq!(
                model.keyboard_input(Keycode::Escape),
                ModelEffect::Activate(MenuCommand::Close)
            );
        }
    }

    #[test]
    fn transfer_suspension_blocks_audio_and_preserves_selection() {
        let mut model = MenuModel::new(&context(false));
        model.selected = 2;
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
        assert_eq!(model.selected, 2);
    }

    #[test]
    fn volume_is_clamped_and_rows_fit() {
        let mut model = MenuModel::new(&context(true));
        model.set_page(MenuPage::System);
        model.audio = Some(AudioState {
            percent: 99,
            muted: false,
        });
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

    #[test]
    fn network_pages_back_and_password_never_escape_to_daemon() {
        let mut model = MenuModel::new(&context(false));
        model.selected = 1;
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
        model.password_col = 3;
        assert_eq!(model.selected_command(), ModelEffect::None); // WPA passphrases need 8 characters
        model.password_col = 2;
        assert_eq!(model.selected_command(), ModelEffect::Redraw);
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
        model.password_col = 3;
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
}
