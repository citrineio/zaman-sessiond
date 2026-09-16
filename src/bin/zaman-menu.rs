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
use tokio::time::{interval, MissedTickBehavior};
use zaman_sessiond::contract::{MenuContextTuple, INTERFACE, MENU_SERVICE, PATH, SERVICE};
use zbus::{connection::Builder, Connection, Proxy};

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
    Shutdown,
}
impl MenuCommand {
    fn action(self) -> &'static str {
        match self {
            Self::Close => "resume",
            Self::ExitGame => "exit-game",
            Self::Shutdown => "shutdown",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Self::Close => "CloseMenu",
            Self::ExitGame => "ExitGame",
            Self::Shutdown => "Shutdown",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MenuItem {
    label: &'static str,
    detail: &'static str,
    command: MenuCommand,
}
fn items(context: &MenuContextTuple) -> Vec<MenuItem> {
    context
        .6
        .iter()
        .filter_map(|action| match action.as_str() {
            "resume" => Some(MenuItem {
                label: "Resume",
                detail: if context.2 == "game" {
                    "Return to current game"
                } else {
                    "Return to game library"
                },
                command: MenuCommand::Close,
            }),
            "exit-game" => Some(MenuItem {
                label: "Exit Game",
                detail: "Close the current game session",
                command: MenuCommand::ExitGame,
            }),
            "shutdown" => Some(MenuItem {
                label: "Shut Down",
                detail: "Exit game and power off.",
                command: MenuCommand::Shutdown,
            }),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelEffect {
    None,
    Redraw,
    Activate(MenuCommand),
}
#[derive(Debug)]
struct MenuModel {
    generation: u64,
    selected: usize,
    pressed: BTreeSet<String>,
    items: Vec<MenuItem>,
}
impl MenuModel {
    fn new(context: &MenuContextTuple) -> Self {
        Self {
            generation: context.1,
            selected: 0,
            pressed: BTreeSet::new(),
            items: items(context),
        }
    }
    fn selected_command(&self) -> ModelEffect {
        self.items
            .get(self.selected)
            .map(|item| ModelEffect::Activate(item.command))
            .unwrap_or(ModelEffect::None)
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
        ModelEffect::Redraw
    }
    fn normalized_input(&mut self, event: &str, value: f64) -> ModelEffect {
        // Keep ALL interception for the complete click, as in v0.6.2.
        if value <= 0.5 {
            if !self.pressed.remove(event) {
                return ModelEffect::None;
            }
            return match event {
                "ui_accept" => self.selected_command(),
                "ui_back" | "ui_cancel" => ModelEffect::Activate(MenuCommand::Close),
                _ => ModelEffect::None,
            };
        }
        if !self.pressed.insert(event.to_string()) {
            return ModelEffect::None;
        }
        match event {
            "ui_up" | "ui_left" => self.navigate(true),
            "ui_down" | "ui_right" => self.navigate(false),
            _ => ModelEffect::None,
        }
    }
    fn keyboard_input(&mut self, keycode: Keycode) -> ModelEffect {
        match keycode {
            Keycode::Up | Keycode::Left => self.navigate(true),
            Keycode::Down | Keycode::Right => self.navigate(false),
            Keycode::Return | Keycode::Space => self.selected_command(),
            Keycode::Escape => ModelEffect::Activate(MenuCommand::Close),
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
        let next = items(&context);
        if self.model.items != next {
            self.model.items = next;
            self.model.selected = self
                .model
                .selected
                .min(self.model.items.len().saturating_sub(1));
            self.model.pressed.clear();
        }
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

        for (index, item) in self.model.items.iter().enumerate() {
            let y = py(369.0) + (index as f32 * 183.0 * scale).round() as i32;
            let selected = index == self.model.selected;
            self.canvas
                .set_draw_color(if selected { SAND } else { JET });
            self.canvas
                .fill_rect(Rect::new(px(96.0), y, pw(978.0), ph(156.0)))
                .map_err(other)?;
            if !selected {
                self.canvas.set_draw_color(CARAMEL);
                self.canvas
                    .draw_rect(Rect::new(px(96.0), y, pw(978.0), ph(156.0)))
                    .map_err(other)?;
            }
            let color = if selected { JET } else { BONE };
            draw_text(
                &mut self.canvas,
                &label,
                item.label,
                color,
                px(135.0),
                y + (13.0 * scale).round() as i32,
            )?;
            draw_text(
                &mut self.canvas,
                &detail,
                item.detail,
                color,
                px(138.0),
                y + (99.0 * scale).round() as i32,
            )?;
            if selected {
                draw_text(
                    &mut self.canvas,
                    &detail,
                    ">",
                    JET,
                    px(1011.0),
                    y + (63.0 * scale).round() as i32,
                )?;
            }
        }

        let (state_label, state_detail) = match self.context.3.as_str() {
            "paused" => ("GAME PAUSED", "Ready when you are."),
            "running" => ("GAME RUNNING", "Your session is active."),
            "unknown" => ("RECOVERY NEEDED", "Resume to try again."),
            _ => ("GAME LIBRARY", "Choose your next memory."),
        };
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
            state_detail,
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
        if self.pending() {
            draw_text(
                &mut self.canvas,
                &detail,
                "Applying selection...",
                SAND,
                px(99.0),
                py(777.0),
            )?;
        } else if error.is_some() {
            // Detailed service diagnostics remain in status/journal; keep the
            // on-screen recovery instruction short and readable at TV distance.
            draw_wrapped(
                &mut self.canvas,
                &detail,
                "Action could not complete. Please try again.",
                SAND,
                px(99.0),
                py(777.0),
                pw(975.0),
                ph(96.0),
            )?;
        }

        self.canvas.set_draw_color(CARAMEL);
        self.canvas
            .fill_rect(Rect::new(px(96.0), py(936.0), pw(1728.0), ph(2.0)))
            .map_err(other)?;
        draw_text(
            &mut self.canvas,
            &small,
            "D-PAD  Navigate     A  Select     B / GUIDE  Resume",
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
    let mut tick = interval(Duration::from_millis(16));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    println!("zaman-menu ready; embedded Saira Condensed / IBM Plex Mono");
    loop {
        let mut effect = ModelEffect::None;
        tokio::select! {
            signal=opened.next()=>{ signal.ok_or_else(||other("MenuOpened stream closed"))?; synchronize(&proxy,&video,ttf,&fonts,&mut surface).await?; }
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
            _=tick.tick()=>{
                for event in pump.poll_iter() {
                    match event {
                        Event::Quit{..}=>return Ok(()),
                        Event::KeyDown{keycode:Some(key),repeat:false,..}=>{
                            if let Some(current)=surface.as_mut().filter(|s| !s.pending()) {
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
        return Err(other("usage: zaman-menu --preview library|game|exit|shutdown|error|pending WIDTH HEIGHT /absolute/output.bmp STATE").into());
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
            vec!["resume".into(), "exit-game".into(), "shutdown".into()]
        } else {
            vec!["resume".into(), "shutdown".into()]
        },
    );
    let mut model = MenuModel::new(&context);
    match args[1].as_str() {
        "exit" => model.selected = 1,
        "shutdown" => model.selected = if game { 2 } else { 1 },
        _ => {}
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
            if game { "paused" } else { "idle" }.into(),
            false,
            String::new(),
            if game {
                vec!["resume".into(), "exit-game".into(), "shutdown".into()]
            } else {
                vec!["resume".into(), "shutdown".into()]
            },
        )
    }

    #[test]
    fn library_lists_resume_and_shutdown() {
        let items = items(&context(false));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].command, MenuCommand::Close);
        assert_eq!(items[1].command, MenuCommand::Shutdown);
        assert_eq!(items[1].label, "Shut Down");
    }

    #[test]
    fn game_lists_resume_exit_and_shutdown() {
        let items = items(&context(true));
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].command, MenuCommand::Close);
        assert_eq!(items[1].command, MenuCommand::ExitGame);
        assert_eq!(items[2].command, MenuCommand::Shutdown);
    }

    #[test]
    fn no_actions_are_invented_when_service_omits_them() {
        let mut c = context(true);
        c.6.clear();
        assert!(items(&c).is_empty());
    }

    #[test]
    fn accept_activates_once_on_release() {
        let mut m = MenuModel::new(&context(true));
        for _ in 0..10 {
            assert_eq!(m.normalized_input("ui_accept", 1.0), ModelEffect::None);
            assert_eq!(m.normalized_input("ui_accept", 1.0), ModelEffect::None);
            assert_eq!(
                m.normalized_input("ui_accept", 0.0),
                ModelEffect::Activate(MenuCommand::Close)
            );
            assert_eq!(m.normalized_input("ui_accept", 0.0), ModelEffect::None);
        }
    }

    #[test]
    fn exit_is_selected_by_navigation_and_activated_only_on_release() {
        let mut m = MenuModel::new(&context(true));
        m.normalized_input("ui_down", 1.0);
        m.normalized_input("ui_down", 0.0);
        assert_eq!(m.normalized_input("ui_accept", 1.0), ModelEffect::None);
        assert_eq!(
            m.normalized_input("ui_accept", 0.0),
            ModelEffect::Activate(MenuCommand::ExitGame)
        );
    }

    #[test]
    fn shutdown_is_reachable_by_navigation_in_game_context() {
        let mut m = MenuModel::new(&context(true));
        m.normalized_input("ui_down", 1.0);
        m.normalized_input("ui_down", 0.0);
        m.normalized_input("ui_down", 1.0);
        m.normalized_input("ui_down", 0.0);
        assert_eq!(m.normalized_input("ui_accept", 1.0), ModelEffect::None);
        assert_eq!(
            m.normalized_input("ui_accept", 0.0),
            ModelEffect::Activate(MenuCommand::Shutdown)
        );
    }

    #[test]
    fn shutdown_is_the_second_item_in_library_context() {
        let mut m = MenuModel::new(&context(false));
        m.normalized_input("ui_down", 1.0);
        m.normalized_input("ui_down", 0.0);
        assert_eq!(m.normalized_input("ui_accept", 1.0), ModelEffect::None);
        assert_eq!(
            m.normalized_input("ui_accept", 0.0),
            ModelEffect::Activate(MenuCommand::Shutdown)
        );
    }

    #[test]
    fn back_cancel_escape_always_request_close() {
        let mut m = MenuModel::new(&context(true));
        m.selected = 2;
        for name in ["ui_back", "ui_cancel"] {
            assert_eq!(m.normalized_input(name, 1.0), ModelEffect::None);
            assert_eq!(
                m.normalized_input(name, 0.0),
                ModelEffect::Activate(MenuCommand::Close)
            );
            assert_eq!(m.normalized_input(name, 0.0), ModelEffect::None);
        }
        assert_eq!(
            m.keyboard_input(Keycode::Escape),
            ModelEffect::Activate(MenuCommand::Close)
        );
    }

    #[test]
    fn navigation_wraps_and_ignores_stale_releases_and_repeated_presses() {
        let mut m = MenuModel::new(&context(true));
        assert_eq!(m.normalized_input("ui_down", 0.0), ModelEffect::None);
        m.normalized_input("ui_up", 1.0);
        assert_eq!(m.selected, 2);
        assert_eq!(m.normalized_input("ui_up", 1.0), ModelEffect::None);
        m.normalized_input("ui_up", 0.0);
        m.normalized_input("ui_up", 1.0);
        assert_eq!(m.selected, 1);

        let mut m = MenuModel::new(&context(false));
        m.navigate(true);
        assert_eq!(m.selected, 1);
    }

    #[test]
    fn command_actions_and_methods_match_dbus_contract() {
        assert_eq!(MenuCommand::Close.action(), "resume");
        assert_eq!(MenuCommand::ExitGame.action(), "exit-game");
        assert_eq!(MenuCommand::Shutdown.action(), "shutdown");
        assert_eq!(MenuCommand::Close.method(), "CloseMenu");
        assert_eq!(MenuCommand::ExitGame.method(), "ExitGame");
        assert_eq!(MenuCommand::Shutdown.method(), "Shutdown");
    }
}
