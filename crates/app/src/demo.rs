//! A clean, reproducible app state for README screenshots and benchmarks,
//! set by environment variables. Inert unless `CATURANKAM_DEMO` is set.
//!
//! - `CATURANKAM_DEMO=1`: default settings that are never saved; no saved login.
//! - `CATURANKAM_DARK=1`: dark appearance.
//! - `CATURANKAM_MOVES="e2e4 e7e5"`: moves to play on the local board.
//! - `CATURANKAM_SHOT=out.png`: capture the window once it has settled, then quit.
//!
//! The window captures itself, so no screen-recording permission is needed.

use eframe::egui;
use std::path::PathBuf;

/// Frames to wait before capturing, so fonts and piece rasters are in.
const SETTLE_FRAMES: u32 = 10;

pub struct Demo {
    pub dark: bool,
    pub moves: String,
    shot: Option<PathBuf>,
    frames: u32,
    requested: bool,
}

impl Demo {
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        var("CATURANKAM_DEMO")?;
        Some(Self {
            dark: var("CATURANKAM_DARK").is_some(),
            moves: var("CATURANKAM_MOVES").unwrap_or_default(),
            shot: var("CATURANKAM_SHOT").map(PathBuf::from),
            frames: 0,
            requested: false,
        })
    }

    /// Call once per frame: requests the capture, then saves it and quits.
    pub fn frame(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.shot else { return };
        self.frames += 1;
        if !self.requested && self.frames >= SETTLE_FRAMES {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.requested = true;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            match image::save_buffer(
                path,
                img.as_raw(),
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            ) {
                Ok(()) => println!("saved {}", path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint(); // keep frames coming until the capture lands
    }
}
