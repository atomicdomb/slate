//! Colour themes: the bundled set, any `.tmTheme` files the user drops in
//! the themes folder, and the editor chrome colours derived from a theme.

use std::path::Path;

use eframe::egui::Color32;
use syntect::highlighting::{Color, Theme, ThemeSet};
use two_face::theme::EmbeddedThemeName as Name;

pub const DEFAULT_THEME: &str = "Monokai Extended";

pub struct Themes {
    /// Sorted by name.
    pub list: Vec<(String, Theme)>,
}

impl Themes {
    pub fn load(user_dir: Option<&Path>) -> Self {
        let set = two_face::theme::extra();
        let mut list: Vec<(String, Theme)> = two_face::theme::EmbeddedLazyThemeSet::theme_names()
            .iter()
            // These three map onto terminal palette slots, not real colours.
            .filter(|n| !matches!(n, Name::Ansi | Name::Base16 | Name::Base16_256))
            .map(|n| (n.as_name().to_string(), set.get(*n).clone()))
            .collect();

        if let Some(entries) = user_dir.and_then(|d| std::fs::read_dir(d).ok()) {
            for path in entries.flatten().map(|e| e.path()) {
                if path.extension().and_then(|e| e.to_str()) != Some("tmTheme") {
                    continue;
                }
                match ThemeSet::get_theme(&path) {
                    Ok(theme) => {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        list.retain(|(n, _)| *n != name);
                        list.push((name, theme));
                    }
                    Err(e) => eprintln!("slate: could not load {}: {e}", path.display()),
                }
            }
        }
        list.sort_by_key(|(n, _)| n.to_lowercase());
        Self { list }
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|(n, _)| n.eq_ignore_ascii_case(name))
    }
}

/// Colours for everything that is not syntax: backgrounds, gutter, caret.
#[derive(Clone)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub fg: Color32,
    pub selection: Color32,
    pub line_highlight: Color32,
    pub gutter_fg: Color32,
    pub caret: Color32,
    pub find: Color32,
    /// Sidebar and status bar.
    pub panel: Color32,
    /// Tab strip behind the tabs.
    pub strip: Color32,
    pub dim: Color32,
    pub hover: Color32,
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Flatten a possibly translucent theme colour onto the background.
fn over(c: Color, bg: Color32) -> Color32 {
    mix(bg, Color32::from_rgb(c.r, c.g, c.b), c.a as f32 / 255.0)
}

fn luminance(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let s = &theme.settings;
        let bg = s
            .background
            .map_or(Color32::from_rgb(40, 41, 35), |c| Color32::from_rgb(c.r, c.g, c.b));
        let dark = luminance(bg) < 0.5;
        let fg = s.foreground.map_or(
            if dark { Color32::from_gray(235) } else { Color32::from_gray(30) },
            |c| over(c, bg),
        );
        let toward = |t: f32| mix(bg, fg, t);
        let shade = |t: f32| mix(bg, Color32::BLACK, t);

        let mut selection = s.selection.map_or(toward(0.22), |c| over(c, bg));
        if (luminance(selection) - luminance(bg)).abs() < 0.04 {
            selection = toward(0.22);
        }
        let mut line_highlight = s.line_highlight.map_or(toward(0.06), |c| over(c, bg));
        if line_highlight == selection {
            line_highlight = toward(0.06);
        }
        let mut gutter_fg = s.gutter_foreground.map_or(toward(0.42), |c| over(c, bg));
        if (luminance(gutter_fg) - luminance(bg)).abs() < 0.12 {
            gutter_fg = toward(0.42);
        }

        Self {
            dark,
            bg,
            fg,
            selection,
            line_highlight,
            gutter_fg,
            caret: s.caret.map_or(fg, |c| over(c, bg)),
            find: s.find_highlight.map_or(Color32::from_rgb(255, 200, 60), |c| over(c, bg)),
            panel: if dark { shade(0.22) } else { shade(0.05) },
            strip: if dark { shade(0.34) } else { shade(0.10) },
            dim: toward(0.55),
            hover: toward(0.10),
        }
    }
}
