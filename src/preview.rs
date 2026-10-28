//! The Klein bottle: turning below a command's output while slow work runs
//! ([`Animation`]), and full screen in `spatial preview`.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::{CrosstermBackend, TestBackend},
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::{
    f32::consts::{PI, TAU},
    io::{self, IsTerminal},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const FRAME_TIME: Duration = Duration::from_millis(33);
const TURN_SECONDS: f32 = 20.0 / 9.0;
const BACKGROUND: Color = Color::Rgb(16, 17, 32);
const LAVENDER: Color = Color::Rgb(181, 157, 255);
const ICE: Color = Color::Rgb(123, 207, 246);
const SHADES: [char; 5] = ['·', ':', '+', '*', '#'];
/// Rows the bottle borrows below a command's output.
const INLINE_HEIGHT: u16 = 20;

fn angle(elapsed: Duration) -> f32 {
    0.6 + elapsed.as_secs_f32() % TURN_SECONDS * TAU / TURN_SECONDS
}

#[derive(Clone, Copy, Debug)]
struct Vector(f32, f32, f32);
impl Vector {
    fn minus(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0, self.1 - rhs.1, self.2 - rhs.2)
    }
    fn times(self, scale: f32) -> Self {
        Self(self.0 * scale, self.1 * scale, self.2 * scale)
    }
    fn dot(self, rhs: Self) -> f32 {
        self.0 * rhs.0 + self.1 * rhs.1 + self.2 * rhs.2
    }
    fn cross(self, rhs: Self) -> Self {
        Self(
            self.1 * rhs.2 - self.2 * rhs.1,
            self.2 * rhs.0 - self.0 * rhs.2,
            self.0 * rhs.1 - self.1 * rhs.0,
        )
    }
    fn unit(self) -> Self {
        self.times(self.dot(self).sqrt().max(1e-9).recip())
    }
    fn rotate(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        let (tilt_sin, tilt_cos) = 0.16_f32.sin_cos();
        let x = self.0 * cos + self.2 * sin;
        let z = self.2 * cos - self.0 * sin;
        Self(
            x * tilt_cos - self.1 * tilt_sin,
            x * tilt_sin + self.1 * tilt_cos,
            z,
        )
    }
}

/// Classic Klein-bottle immersion, with a returning neck through its own wall.
///
/// Angles cover [0, 2pi]. At the u seam, v is glued to pi-v: the orientation
/// reverses, unlike a torus. Coordinates are recentered/scaled for rendering.
/// Formula checked against the official Three.js `ParametricFunctions.klein`:
/// <https://github.com/mrdoob/three.js/blob/dev/examples/jsm/geometries/ParametricFunctions.js>
fn klein(u: f32, v: f32) -> Vector {
    let (su, cu) = u.sin_cos();
    let (sv, cv) = v.sin_cos();
    let radius = 2.0 - cu;
    let center = 3.0 * cu * (1.0 + su);
    let (x, up) = if u < PI {
        (center + radius * cu * cv, 8.0 * su + radius * su * cv)
    } else {
        (center - radius * cv, 8.0 * su)
    };
    Vector(x / 9.0, (up - 1.0) / 9.0, -radius * sv / 9.0)
}

struct Sample {
    position: Vector,
    normal: Vector,
}
struct Bottle {
    samples: Vec<Sample>,
}
impl Bottle {
    fn new() -> Self {
        let mut samples = Vec::with_capacity(600 * 220);
        // Sample once; animation only rotates/projects this surface. Dense
        // sampling fills character cells without a GPU or a raster framework.
        for i in 0..600 {
            let u = (i as f32 + 0.5) * TAU / 600.0;
            for j in 0..220 {
                let v = (j as f32 + 0.5) * TAU / 220.0;
                let position = klein(u, v);
                let du = klein(u + 0.0001, v).minus(position);
                let dv = klein(u, v + 0.0001).minus(position);
                samples.push(Sample {
                    position,
                    normal: du.cross(dv).unit(),
                });
            }
        }
        Self { samples }
    }

    fn pixels(&self, width: u16, height: u16, angle: f32) -> Vec<Pixel> {
        let mut pixels = vec![Pixel::default(); usize::from(width) * usize::from(height)];
        if width == 0 || height == 0 {
            return pixels;
        }
        // Character cells are about twice as tall as they are wide. Correct
        // their aspect ratio before projection; keep scale fixed across frames.
        let scale = (f32::from(width) / 4.0).min(f32::from(height) / 2.45);
        let light = Vector(-0.45, 0.65, 0.8).unit();
        let half_light = Vector(-0.25, 0.35, 1.0).unit();
        for sample in &self.samples {
            let point = sample.position.rotate(angle);
            let perspective = 3.8 / (3.8 - point.2);
            let x = (f32::from(width) * 0.5 + point.0 * scale * 2.0 * perspective).floor() as i32;
            let y = (f32::from(height) * 0.5 - point.1 * scale * perspective).floor() as i32;
            if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
                continue;
            }
            let mut normal = sample.normal.rotate(angle);
            // A Klein bottle has no consistent inside/outside. Shade the side
            // facing the camera instead of deleting half the surface by culling.
            if normal.dot(Vector(0.0, 0.0, 3.8).minus(point)) < 0.0 {
                normal = normal.times(-1.0);
            }
            let diffuse = normal.dot(light).max(0.0);
            let highlight = normal.dot(half_light).max(0.0).powi(20);
            let depth_light = (0.86 + point.2 * 0.17).clamp(0.65, 1.0);
            let brightness =
                ((0.13 + 0.72 * diffuse + 0.20 * highlight) * depth_light).clamp(0.0, 1.0);
            let at = y as usize * usize::from(width) + x as usize;
            pixels[at].place(point.2, brightness);
        }
        pixels
    }
}

#[derive(Clone, Copy)]
struct Pixel {
    depth: f32,
    brightness: f32,
}
impl Default for Pixel {
    fn default() -> Self {
        Self {
            depth: f32::NEG_INFINITY,
            brightness: 0.0,
        }
    }
}
impl Pixel {
    fn place(&mut self, depth: f32, brightness: f32) {
        if depth > self.depth {
            *self = Self { depth, brightness };
        }
    }
    fn character(self) -> char {
        if !self.depth.is_finite() {
            return ' ';
        }
        SHADES[((self.brightness * 5.0) as usize).min(SHADES.len() - 1)]
    }
    fn color(self) -> Color {
        let light = self.brightness;
        // Lavender in the shadows; ice-blue highlights. Depth also affects
        // character density, so monochrome snapshots still show the surface.
        let amount = ((light - 0.55) / 0.45).clamp(0.0, 1.0);
        let shade = 0.38 + 0.62 * light;
        let blend = |a: f32, b: f32| ((a + (b - a) * amount) * shade) as u8;
        Color::Rgb(
            blend(181.0, 123.0),
            blend(157.0, 207.0),
            blend(255.0, 246.0),
        )
    }
}

struct Preview {
    elapsed: Duration,
    paused: bool,
    operation: &'static str,
    status: &'static str,
}
impl Default for Preview {
    fn default() -> Self {
        Self {
            elapsed: Duration::ZERO,
            paused: false,
            operation: "spatial build",
            status: "Checking constraints…",
        }
    }
}
impl Preview {
    fn advance(&mut self, elapsed: Duration) {
        if !self.paused {
            self.elapsed += elapsed;
        }
    }
    fn handle(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        match key {
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return false,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => return false,
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('i' | 'I') => {
                self.operation = "spatial";
                self.status = "Ready.";
            }
            KeyCode::Char('n' | 'N') => {
                self.operation = "spatial new hello";
                self.status = "Creating project…";
            }
            KeyCode::Char('b' | 'B') => {
                self.operation = "spatial build";
                self.status = "Checking constraints…";
            }
            KeyCode::Char('r' | 'R') => {
                self.operation = "spatial run";
                self.status = "Running RTL simulation…";
            }
            _ => {}
        }
        true
    }
}

/// Show the single chosen aesthetic. The operation/status are preview examples.
pub(crate) fn run() -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "preview needs an interactive terminal; use `spatial preview --snapshot` for a plain-text frame",
        ));
    }
    let bottle = Bottle::new();
    // ratatui::run restores raw mode, alternate screen and cursor on normal
    // return, I/O error and unwinding. No bespoke terminal cleanup is needed.
    ratatui::run(|terminal| {
        let mut preview = Preview::default();
        let mut last_frame = Instant::now();
        loop {
            terminal.draw(|frame| {
                render(
                    frame,
                    &bottle,
                    angle(preview.elapsed),
                    preview.operation,
                    preview.status,
                );
            })?;
            let wait = FRAME_TIME.saturating_sub(last_frame.elapsed());
            if event::poll(wait)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && !preview.handle(key.code, key.modifiers)
            {
                return Ok(());
            }
            if last_frame.elapsed() >= FRAME_TIME {
                preview.advance(last_frame.elapsed());
                last_frame = Instant::now();
            }
        }
    })
}

/// Print the same renderer without terminal controls or color escape sequences.
pub(crate) fn snapshot() -> String {
    let mut terminal =
        ratatui::Terminal::new(TestBackend::new(104, 42)).expect("in-memory terminal");
    let bottle = Bottle::new();
    terminal
        .draw(|frame| {
            let preview = Preview::default();
            render(
                frame,
                &bottle,
                angle(preview.elapsed),
                preview.operation,
                preview.status,
            );
        })
        .expect("in-memory drawing");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draw the bottle with two lines of text under it, and return where the
/// second line ends.
fn render(
    frame: &mut Frame<'_>,
    bottle: &Bottle,
    angle: f32,
    operation: &str,
    status: &str,
) -> Position {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::new().bg(BACKGROUND)), area);
    if area.width == 0 || area.height == 0 {
        return area.as_position();
    }
    let margin = u16::from(area.width >= 16) * 2;
    let width = area.width.saturating_sub(margin * 2).min(140);
    let height = area.height.saturating_sub(5).min(54);
    let canvas = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + area.height.saturating_sub(height + 4) / 2,
        width,
        height,
    );
    let pixels = bottle.pixels(width, height, angle);
    let rows: Vec<Line<'_>> = pixels
        .chunks(usize::from(width).max(1))
        .map(|row| {
            Line::from(
                row.iter()
                    .map(|pixel| {
                        Span::styled(
                            pixel.character().to_string(),
                            Style::new().fg(pixel.color()),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), canvas);
    let text_x = area.x + area.width.saturating_sub(26) / 2;
    let text_y = if area.height >= 5 {
        canvas.y + height + 1
    } else {
        area.y
    };
    let text = Rect::new(
        text_x,
        text_y,
        area.width.saturating_sub(text_x - area.x),
        area.height.saturating_sub(text_y - area.y).min(2),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(operation, Style::new().fg(LAVENDER)),
            Line::styled(status, Style::new().fg(ICE)),
        ]),
        text,
    );
    let end = text.x.saturating_add(status.chars().count() as u16);
    Position::new(end.min(area.right().saturating_sub(1)), text.y + 1)
}

/// The bottle turning below a command's output until this is dropped, with a
/// status line the work can update. Dropping it erases the bottle and leaves
/// the cursor where the bottle began, so later output reads as if it was never
/// there.
///
/// It draws only when stdout is a terminal with room for it. Otherwise every
/// status is printed as a plain line, so pipes and CI logs get text.
pub(crate) struct Animation {
    shown: Arc<Mutex<Shown>>,
    drawing: Option<thread::JoinHandle<()>>,
}

struct Shown {
    status: String,
    done: bool,
}

impl Animation {
    pub(crate) fn start(operation: &'static str, status: &str) -> Self {
        let shown = Arc::new(Mutex::new(Shown {
            status: status.to_owned(),
            done: false,
        }));
        let room = crossterm::terminal::size().is_ok_and(|(_, rows)| rows >= INLINE_HEIGHT + 4);
        if !io::stdout().is_terminal() || !room {
            println!("  {status}");
            return Self {
                shown,
                drawing: None,
            };
        }
        let drawing = thread::spawn({
            let shown = Arc::clone(&shown);
            // A terminal that stops accepting output just ends the animation.
            move || drop(animate(operation, &shown))
        });
        Self {
            shown,
            drawing: Some(drawing),
        }
    }

    pub(crate) fn status(&self, status: &str) {
        if self.drawing.is_some() {
            self.shown.lock().expect("drawing never panics").status = status.to_owned();
        } else {
            println!("  {status}");
        }
    }
}

impl Drop for Animation {
    fn drop(&mut self) {
        self.shown.lock().expect("drawing never panics").done = true;
        if let Some(drawing) = self.drawing.take() {
            let _ = drawing.join();
        }
    }
}

fn animate(operation: &str, shown: &Mutex<Shown>) -> io::Result<()> {
    let bottle = Bottle::new();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(io::stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(INLINE_HEIGHT),
        },
    )?;
    let started = Instant::now();
    loop {
        let (status, done) = {
            let shown = shown.lock().expect("drawing never panics");
            (shown.status.clone(), shown.done)
        };
        if done {
            break;
        }
        let elapsed = started.elapsed();
        let status = format!("{status}  {}s", elapsed.as_secs());
        terminal.draw(|frame| {
            // Leave the cursor visible at the end of the status, so that an
            // interrupt never strands the terminal with a hidden cursor.
            let end = render(frame, &bottle, angle(elapsed), operation, &status);
            frame.set_cursor_position(end);
        })?;
        thread::sleep(FRAME_TIME);
    }
    terminal.clear()?;
    let top = terminal.get_frame().area().as_position();
    terminal.set_cursor_position(top)
}
