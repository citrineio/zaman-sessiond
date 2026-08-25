use futures_util::StreamExt;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, TextureQuery};
use sdl2::ttf::{Font, Sdl2TtfContext};
use sdl2::video::Window;
use std::collections::BTreeSet;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};
use zaman_sessiond::contract::{INTERFACE, PATH, SERVICE};
use zbus::{Connection, Proxy};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

const MENU_ITEMS: [MenuItem; 2] = [
    MenuItem {
        label: "Resume",
        detail: "Return to the current game",
        command: MenuCommand::Resume,
    },
    MenuItem {
        label: "Exit Game",
        detail: "Close the current game session",
        command: MenuCommand::ExitGame,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuCommand {
    Resume,
    ExitGame,
}

impl MenuCommand {
    fn method(self) -> &'static str {
        match self {
            Self::Resume => "Resume",
            Self::ExitGame => "ExitGame",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MenuItem {
    label: &'static str,
    detail: &'static str,
    command: MenuCommand,
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
}

impl MenuModel {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            selected: 0,
            pressed: BTreeSet::new(),
        }
    }

    fn normalized_input(&mut self, event: &str, value: f64) -> ModelEffect {
        if value <= 0.5 {
            self.pressed.remove(event);
            return ModelEffect::None;
        }

        if !self.pressed.insert(event.to_string()) {
            return ModelEffect::None;
        }

        match event {
            "ui_up" | "ui_left" => {
                self.selected = if self.selected == 0 {
                    MENU_ITEMS.len() - 1
                } else {
                    self.selected - 1
                };
                ModelEffect::Redraw
            }
            "ui_down" | "ui_right" => {
                self.selected = (self.selected + 1) % MENU_ITEMS.len();
                ModelEffect::Redraw
            }
            "ui_accept" => ModelEffect::Activate(MENU_ITEMS[self.selected].command),
            "ui_back" | "ui_cancel" => ModelEffect::Activate(MenuCommand::Resume),
            _ => ModelEffect::None,
        }
    }

    fn keyboard_input(&mut self, keycode: Keycode) -> ModelEffect {
        match keycode {
            Keycode::Up | Keycode::Left => {
                self.selected = if self.selected == 0 {
                    MENU_ITEMS.len() - 1
                } else {
                    self.selected - 1
                };
                ModelEffect::Redraw
            }
            Keycode::Down | Keycode::Right => {
                self.selected = (self.selected + 1) % MENU_ITEMS.len();
                ModelEffect::Redraw
            }
            Keycode::Return | Keycode::Space => {
                ModelEffect::Activate(MENU_ITEMS[self.selected].command)
            }
            Keycode::Escape => ModelEffect::Activate(MenuCommand::Resume),
            _ => ModelEffect::None,
        }
    }
}

struct MenuSurface {
    canvas: Canvas<Window>,
    model: MenuModel,
    pending: bool,
    error: Option<String>,
}

impl MenuSurface {
    fn open(
        video: &sdl2::VideoSubsystem,
        ttf: &Sdl2TtfContext,
        font_path: &Path,
        generation: u64,
    ) -> Result<Self> {
        let window = video
            .window("Zaman System Menu", 1280, 720)
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
        let mut surface = Self {
            canvas,
            model: MenuModel::new(generation),
            pending: false,
            error: None,
        };
        surface.render(ttf, font_path)?;
        Ok(surface)
    }

    fn normalized_input(
        &mut self,
        ttf: &Sdl2TtfContext,
        font_path: &Path,
        event: &str,
        value: f64,
    ) -> Result<ModelEffect> {
        let effect = self.model.normalized_input(event, value);
        if effect == ModelEffect::Redraw {
            self.render(ttf, font_path)?;
        }
        Ok(effect)
    }

    fn keyboard_input(
        &mut self,
        ttf: &Sdl2TtfContext,
        font_path: &Path,
        keycode: Keycode,
    ) -> Result<ModelEffect> {
        if self.pending {
            return Ok(ModelEffect::None);
        }
        let effect = self.model.keyboard_input(keycode);
        if effect == ModelEffect::Redraw {
            self.render(ttf, font_path)?;
        }
        Ok(effect)
    }

    fn set_pending(&mut self, ttf: &Sdl2TtfContext, font_path: &Path, pending: bool) -> Result<()> {
        self.pending = pending;
        self.render(ttf, font_path)
    }

    fn set_error(&mut self, ttf: &Sdl2TtfContext, font_path: &Path, error: String) -> Result<()> {
        self.pending = false;
        self.error = Some(error);
        self.render(ttf, font_path)
    }

    fn render(&mut self, ttf: &Sdl2TtfContext, font_path: &Path) -> Result<()> {
        let (width, height) = self.canvas.output_size().map_err(other)?;
        let scale = (height as f32 / 1080.0).clamp(0.65, 2.0);
        let margin = (88.0 * scale) as i32;
        let title_size = (58.0 * scale).round() as u16;
        let subtitle_size = (24.0 * scale).round() as u16;
        let item_size = (38.0 * scale).round() as u16;
        let detail_size = (21.0 * scale).round() as u16;
        let footer_size = (20.0 * scale).round() as u16;
        let title_font = ttf.load_font(font_path, title_size).map_err(other)?;
        let subtitle_font = ttf.load_font(font_path, subtitle_size).map_err(other)?;
        let item_font = ttf.load_font(font_path, item_size).map_err(other)?;
        let detail_font = ttf.load_font(font_path, detail_size).map_err(other)?;
        let footer_font = ttf.load_font(font_path, footer_size).map_err(other)?;

        self.canvas.set_draw_color(Color::RGB(9, 13, 22));
        self.canvas.clear();

        self.canvas.set_draw_color(Color::RGB(14, 22, 35));
        self.canvas
            .fill_rect(Rect::new(0, 0, (width as f32 * 0.62) as u32, height))
            .map_err(other)?;
        self.canvas.set_draw_color(Color::RGB(33, 211, 197));
        self.canvas
            .fill_rect(Rect::new(0, 0, (10.0 * scale) as u32, height))
            .map_err(other)?;

        draw_text(
            &mut self.canvas,
            &title_font,
            "ZAMAN",
            Color::RGB(239, 246, 255),
            margin,
            margin,
        )?;
        draw_text(
            &mut self.canvas,
            &subtitle_font,
            "SYSTEM MENU",
            Color::RGB(33, 211, 197),
            margin,
            margin + (78.0 * scale) as i32,
        )?;

        let item_x = margin;
        let item_width = ((width as f32 * 0.46) - margin as f32).max(420.0 * scale) as u32;
        let item_height = (116.0 * scale) as u32;
        let item_gap = (18.0 * scale) as i32;
        let first_y = (height as f32 * 0.39) as i32;

        for (index, item) in MENU_ITEMS.iter().enumerate() {
            let y = first_y + index as i32 * (item_height as i32 + item_gap);
            let selected = index == self.model.selected;
            self.canvas.set_draw_color(if selected {
                Color::RGB(33, 211, 197)
            } else {
                Color::RGB(25, 35, 51)
            });
            self.canvas
                .fill_rect(Rect::new(item_x, y, item_width, item_height))
                .map_err(other)?;

            let label_color = if selected {
                Color::RGB(5, 24, 29)
            } else {
                Color::RGB(235, 242, 250)
            };
            let detail_color = if selected {
                Color::RGB(13, 61, 64)
            } else {
                Color::RGB(151, 166, 184)
            };
            draw_text(
                &mut self.canvas,
                &item_font,
                item.label,
                label_color,
                item_x + (28.0 * scale) as i32,
                y + (18.0 * scale) as i32,
            )?;
            draw_text(
                &mut self.canvas,
                &detail_font,
                item.detail,
                detail_color,
                item_x + (30.0 * scale) as i32,
                y + (70.0 * scale) as i32,
            )?;
        }

        let footer = if self.pending {
            "Applying selection..."
        } else if let Some(error) = self.error.as_deref() {
            error
        } else {
            "NAVIGATE     ACCEPT  Select     BACK  Resume"
        };
        let footer_color = if self.error.is_some() {
            Color::RGB(255, 128, 128)
        } else {
            Color::RGB(151, 166, 184)
        };
        draw_text(
            &mut self.canvas,
            &footer_font,
            footer,
            footer_color,
            margin,
            height as i32 - margin,
        )?;

        self.canvas.present();
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        std::env::set_var("SDL_VIDEODRIVER", "wayland");
    }

    let font_path = find_font()?;
    let sdl = sdl2::init().map_err(other)?;
    let video = sdl.video().map_err(other)?;
    let ttf = sdl2::ttf::init().map_err(other)?;
    let mut event_pump = sdl.event_pump().map_err(other)?;
    let connection = Connection::session().await?;
    let proxy = Proxy::new(&connection, SERVICE, PATH, INTERFACE).await?;

    let mut opened = proxy.receive_signal("MenuOpened").await?;
    let mut closed = proxy.receive_signal("MenuClosed").await?;
    let mut input = proxy.receive_signal("MenuInput").await?;
    let status: (bool, u64, String) = proxy.call("MenuStatus", &()).await?;
    let mut surface = if status.0 {
        println!(
            "Synchronizing already-open menu generation={} reason={}.",
            status.1, status.2
        );
        Some(MenuSurface::open(&video, &ttf, &font_path, status.1)?)
    } else {
        None
    };

    println!("zaman-menu ready; font={}", font_path.display());
    let mut event_tick = interval(Duration::from_millis(16));
    event_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            message = opened.next() => {
                let message = message.ok_or_else(|| other("MenuOpened signal stream closed"))?;
                let (generation, reason): (u64, String) = message.body().deserialize()?;
                println!("Opening menu generation={generation} reason={reason}.");
                surface = Some(MenuSurface::open(&video, &ttf, &font_path, generation)?);
            }
            message = closed.next() => {
                let message = message.ok_or_else(|| other("MenuClosed signal stream closed"))?;
                let (generation, reason): (u64, String) = message.body().deserialize()?;
                if surface.as_ref().is_some_and(|current| current.model.generation == generation) {
                    println!("Closing menu generation={generation} reason={reason}.");
                    surface = None;
                }
            }
            message = input.next() => {
                let message = message.ok_or_else(|| other("MenuInput signal stream closed"))?;
                let (generation, event, value): (u64, String, f64) = message.body().deserialize()?;
                let effect = match surface.as_mut() {
                    Some(current) if current.model.generation == generation => {
                        current.normalized_input(&ttf, &font_path, &event, value)?
                    }
                    _ => ModelEffect::None,
                };
                activate(&proxy, &ttf, &font_path, surface.as_mut(), effect).await?;
            }
            _ = event_tick.tick() => {
                let mut effect = ModelEffect::None;
                for event in event_pump.poll_iter() {
                    match event {
                        Event::Quit { .. } => return Ok(()),
                        Event::KeyDown { keycode: Some(keycode), repeat: false, .. } => {
                            if let Some(current) = surface.as_mut() {
                                effect = current.keyboard_input(&ttf, &font_path, keycode)?;
                                if effect != ModelEffect::None {
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                activate(&proxy, &ttf, &font_path, surface.as_mut(), effect).await?;
            }
        }
    }
}

async fn activate(
    proxy: &Proxy<'_>,
    ttf: &Sdl2TtfContext,
    font_path: &Path,
    surface: Option<&mut MenuSurface>,
    effect: ModelEffect,
) -> Result<()> {
    let ModelEffect::Activate(command) = effect else {
        return Ok(());
    };
    let Some(surface) = surface else {
        return Ok(());
    };
    if surface.pending {
        return Ok(());
    }

    surface.set_pending(ttf, font_path, true)?;
    println!("Activating {}.", command.method());
    let result: zbus::Result<()> = proxy.call(command.method(), &()).await;
    if let Err(error) = result {
        eprintln!("{} failed: {error}", command.method());
        surface.set_error(ttf, font_path, format!("Selection failed: {error}"))?;
    }
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
    let texture_creator = canvas.texture_creator();
    let texture = texture_creator
        .create_texture_from_surface(&surface)
        .map_err(other)?;
    let TextureQuery { width, height, .. } = texture.query();
    canvas
        .copy(&texture, None, Rect::new(x, y, width, height))
        .map_err(other)?;
    Ok(())
}

fn find_font() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("ZAMAN_MENU_FONT") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(other(format!(
            "ZAMAN_MENU_FONT is not a regular file: {}",
            path.display()
        ))
        .into());
    }

    FONT_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .ok_or_else(|| other("no supported Zaman menu font was found").into())
}

fn other(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{MenuCommand, MenuModel, ModelEffect};

    #[test]
    fn accept_activates_the_selected_item_once_per_press() {
        let mut model = MenuModel::new(7);

        assert_eq!(
            model.normalized_input("ui_accept", 1.0),
            ModelEffect::Activate(MenuCommand::Resume)
        );
        assert_eq!(model.normalized_input("ui_accept", 1.0), ModelEffect::None);
        assert_eq!(model.normalized_input("ui_accept", 0.0), ModelEffect::None);
        assert_eq!(
            model.normalized_input("ui_accept", 1.0),
            ModelEffect::Activate(MenuCommand::Resume)
        );
    }

    #[test]
    fn navigation_wraps_and_exit_is_the_second_item() {
        let mut model = MenuModel::new(1);

        assert_eq!(model.normalized_input("ui_up", 1.0), ModelEffect::Redraw);
        model.normalized_input("ui_up", 0.0);
        assert_eq!(
            model.normalized_input("ui_accept", 1.0),
            ModelEffect::Activate(MenuCommand::ExitGame)
        );
    }

    #[test]
    fn back_always_requests_resume() {
        let mut model = MenuModel::new(1);
        model.normalized_input("ui_down", 1.0);
        model.normalized_input("ui_down", 0.0);

        assert_eq!(
            model.normalized_input("ui_back", 1.0),
            ModelEffect::Activate(MenuCommand::Resume)
        );
    }

    #[test]
    fn stale_release_does_not_move_selection() {
        let mut model = MenuModel::new(1);

        assert_eq!(model.normalized_input("ui_down", 0.0), ModelEffect::None);
        assert_eq!(model.selected, 0);
    }
}
