//! Sidebar icons, drawn from a sprite sheet.
//!
//! The sheet is a grid `COLUMNS` wide of square cells; the cell size is
//! whatever the image width divides into, so 16, 32 or 64 px art all work.
//! Cells are read left to right, top to bottom, in the order of [`Icon`].
//!
//! A built-in sheet is compiled in. An `icons.png` in the settings folder
//! replaces it cell by cell: any cell left fully transparent (or missing
//! because the sheet has fewer rows) keeps the built-in icon.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{pos2, Color32, ColorImage, Context, Painter, Rect, TextureHandle, TextureOptions};

pub const COLUMNS: usize = 8;
pub const FILE_NAME: &str = "icons.png";
const BUILT_IN: &[u8] = include_bytes!("../assets/icons.png");

/// One slot in the sheet. The order here IS the order in the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Folder,
    FolderOpen,
    File,
    Rust,
    Toml,
    Markdown,
    Json,
    Yaml,
    Text,
    Image,
    Shell,
    Lock,
    Git,
    Config,
    Shader,
    C,
    Cpp,
    Python,
    JavaScript,
    TypeScript,
    Html,
    Css,
    Audio,
    Archive,
}

impl Icon {
    /// Which icon a file gets, from its name.
    pub fn for_file(name: &str) -> Self {
        let lower = name.to_lowercase();
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        if lower.starts_with(".git") {
            return Icon::Git;
        }
        match ext {
            "rs" => Icon::Rust,
            "toml" => Icon::Toml,
            "md" | "markdown" => Icon::Markdown,
            "json" | "jsonc" | "ron" => Icon::Json,
            "yml" | "yaml" => Icon::Yaml,
            "txt" | "log" => Icon::Text,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tga" | "psd"
            | "aseprite" => Icon::Image,
            "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" | "ps1" => Icon::Shell,
            "lock" => Icon::Lock,
            "ini" | "cfg" | "conf" | "env" | "xml" | "plist" | "editorconfig" => Icon::Config,
            "glsl" | "wgsl" | "hlsl" | "vert" | "frag" | "comp" | "shader" => Icon::Shader,
            "c" | "h" => Icon::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" => Icon::Cpp,
            "py" => Icon::Python,
            "js" | "mjs" | "cjs" | "jsx" => Icon::JavaScript,
            "ts" | "tsx" => Icon::TypeScript,
            "html" | "htm" => Icon::Html,
            "css" | "scss" | "sass" | "less" => Icon::Css,
            "wav" | "ogg" | "mp3" | "flac" => Icon::Audio,
            "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" | "jpak" => Icon::Archive,
            _ if matches!(lower.as_str(), "license" | "readme" | "authors") => Icon::Text,
            _ => Icon::File,
        }
    }
}

struct Sheet {
    texture: TextureHandle,
    rows: usize,
    /// Per slot: does the cell contain anything?
    filled: Vec<bool>,
}

impl Sheet {
    fn load(ctx: &Context, name: &str, bytes: &[u8]) -> Result<Self, String> {
        let rgba = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let cell = w / COLUMNS;
        if cell == 0 || w % COLUMNS != 0 {
            return Err(format!("width {w} is not a multiple of {COLUMNS} columns"));
        }
        let rows = h / cell;
        if rows == 0 {
            return Err(format!("height {h} is less than one {cell} px row"));
        }
        let filled = (0..rows * COLUMNS)
            .map(|slot| {
                let (x0, y0) = (slot % COLUMNS * cell, slot / COLUMNS * cell);
                (y0..y0 + cell).any(|y| (x0..x0 + cell).any(|x| rgba.get_pixel(x as u32, y as u32)[3] > 8))
            })
            .collect();
        let pixels = ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw());
        Ok(Self { texture: ctx.load_texture(name, pixels, TextureOptions::LINEAR), rows, filled })
    }

    fn has(&self, slot: usize) -> bool {
        self.filled.get(slot).copied().unwrap_or(false)
    }

    fn paint(&self, painter: &Painter, slot: usize, rect: Rect) {
        let (col, row) = ((slot % COLUMNS) as f32, (slot / COLUMNS) as f32);
        let (cols, rows) = (COLUMNS as f32, self.rows as f32);
        // Pull in by half a texel so neighbouring cells never bleed in.
        let [tw, th] = self.texture.size();
        let (ix, iy) = (0.5 / tw as f32, 0.5 / th as f32);
        let uv = Rect::from_min_max(
            pos2(col / cols + ix, row / rows + iy),
            pos2((col + 1.0) / cols - ix, (row + 1.0) / rows - iy),
        );
        painter.image(self.texture.id(), rect, uv, Color32::WHITE);
    }
}

pub struct Icons {
    built_in: Sheet,
    custom: Option<Sheet>,
    custom_path: Option<PathBuf>,
    custom_seen: Option<SystemTime>,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Icons {
    /// `dir` is the settings folder that may hold a replacement sheet.
    pub fn load(ctx: &Context, dir: Option<&Path>) -> Self {
        let built_in = Sheet::load(ctx, "icons", BUILT_IN).expect("built-in icon sheet is valid");
        let mut icons = Self {
            built_in,
            custom: None,
            custom_path: dir.map(|d| d.join(FILE_NAME)),
            custom_seen: None,
        };
        icons.reload(ctx);
        icons
    }

    /// Pick up a replacement sheet that was added, edited or removed.
    /// Returns a message if the file is there but unusable.
    pub fn reload(&mut self, ctx: &Context) -> Option<String> {
        let path = self.custom_path.as_ref()?;
        let stamp = modified(path);
        if stamp == self.custom_seen {
            return None;
        }
        self.custom_seen = stamp;
        self.custom = None;
        stamp?;
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Sheet::load(ctx, "custom-icons", &bytes));
        match loaded {
            Ok(sheet) => {
                self.custom = Some(sheet);
                None
            }
            Err(e) => Some(format!("{FILE_NAME} ignored: {e}")),
        }
    }

    pub fn paint(&self, painter: &Painter, icon: Icon, rect: Rect) {
        let slot = icon as usize;
        match &self.custom {
            Some(sheet) if sheet.has(slot) => sheet.paint(painter, slot, rect),
            _ => self.built_in.paint(painter, slot, rect),
        }
    }
}

/// Write the built-in sheet into `dir` as a starting point for editing.
pub fn export(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(FILE_NAME);
    if path.exists() {
        return Err(format!("{} already exists; move it away first", path.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, BUILT_IN).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_map_to_icons() {
        assert_eq!(Icon::for_file("loot.rs"), Icon::Rust);
        assert_eq!(Icon::for_file("bake-punkhazzard.toml"), Icon::Toml);
        assert_eq!(Icon::for_file("ZONES.md"), Icon::Markdown);
        assert_eq!(Icon::for_file("Cargo.lock"), Icon::Lock);
        assert_eq!(Icon::for_file(".gitignore"), Icon::Git);
        assert_eq!(Icon::for_file("vendor-assets.sh"), Icon::Shell);
        assert_eq!(Icon::for_file("zone-0.jpak"), Icon::Archive);
        assert_eq!(Icon::for_file(".DS_Store"), Icon::File);
        assert_eq!(Icon::for_file("LICENSE"), Icon::Text);
    }

    #[test]
    fn built_in_sheet_fills_every_slot() {
        let rgba = image::load_from_memory(BUILT_IN).unwrap().to_rgba8();
        let cell = rgba.width() as usize / COLUMNS;
        assert_eq!(rgba.width() as usize % COLUMNS, 0);
        let slots = rgba.height() as usize / cell * COLUMNS;
        assert!(slots > Icon::Archive as usize);
    }
}
