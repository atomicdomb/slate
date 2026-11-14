//! The folder sidebar and the file index behind quick-open.

use std::path::{Path, PathBuf};

use eframe::egui::{self, pos2, vec2, Align2, FontId, Rect, Sense, Ui};

use crate::icons::{Icon, Icons};
use crate::theme::Palette;

const ROW: f32 = 23.0;
/// Icons are drawn at this size whatever the sheet's resolution.
const ICON: f32 = 16.0;
/// Folders that are never worth indexing for quick-open.
const SKIP: &[&str] = &[".git", "target", "node_modules", ".DS_Store"];
const INDEX_LIMIT: usize = 40_000;

pub struct Node {
    pub path: PathBuf,
    name: String,
    is_dir: bool,
    icon: Icon,
    open: bool,
    /// Read from disk the first time the folder is opened.
    children: Option<Vec<Node>>,
}

fn read_dir(dir: &Path) -> Vec<Node> {
    let mut nodes: Vec<Node> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            Node {
                icon: Icon::for_file(&name),
                name,
                is_dir: path.is_dir(),
                open: false,
                children: None,
                path,
            }
        })
        .filter(|n| n.name != ".DS_Store" && n.name != ".git")
        .collect();
    nodes.sort_by_key(|n| (!n.is_dir, n.name.to_lowercase()));
    nodes
}

impl Node {
    pub fn root(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map_or_else(|| path.to_string_lossy().to_string(), |n| n.to_string_lossy().to_string());
        Self {
            children: Some(read_dir(&path)),
            name,
            is_dir: true,
            icon: Icon::Folder,
            open: true,
            path,
        }
    }

    /// Re-read every folder that is currently loaded, keeping what is open.
    pub fn refresh(&mut self) {
        let Some(mut old) = self.children.take() else { return };
        let mut fresh = read_dir(&self.path);
        for n in &mut fresh {
            if let Some(prev) = old.iter_mut().find(|o| o.path == n.path) {
                n.open = prev.open;
                n.children = prev.children.take();
                n.refresh();
            }
        }
        self.children = Some(fresh);
    }

    /// Draw this node and its open descendants. Returns a clicked file.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        active: Option<&Path>,
        pal: &Palette,
        icons: &Icons,
    ) -> Option<PathBuf> {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        let is_active = !self.is_dir && active == Some(self.path.as_path());
        let painter = ui.painter();
        if is_active || resp.hovered() {
            painter.rect_filled(rect, 0.0, if is_active { pal.selection } else { pal.hover });
        }
        let x = rect.left() + 12.0 + depth as f32 * 16.0;
        let mid = rect.center().y;
        let icon = match (self.is_dir, self.open) {
            (true, true) => Icon::FolderOpen,
            (true, false) => Icon::Folder,
            (false, _) => self.icon,
        };
        icons.paint(painter, icon, Rect::from_min_size(pos2(x, mid - ICON / 2.0), vec2(ICON, ICON)));
        painter.with_clip_rect(Rect::from_min_max(rect.min, pos2(ui.clip_rect().right(), rect.bottom()))).text(
            pos2(x + ICON + 7.0, mid),
            Align2::LEFT_CENTER,
            &self.name,
            FontId::proportional(13.0),
            if is_active || self.is_dir { pal.fg } else { pal.fg.gamma_multiply(0.82) },
        );

        let mut clicked = None;
        if resp.clicked() {
            if self.is_dir {
                self.open = !self.open;
            } else {
                clicked = Some(self.path.clone());
            }
        }
        if self.is_dir && self.open {
            let path = &self.path;
            for child in self.children.get_or_insert_with(|| read_dir(path)) {
                clicked = child.show(ui, depth + 1, active, pal, icons).or(clicked);
            }
        }
        clicked
    }
}

/// Every file under `root`, as paths relative to it.
pub fn index(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if SKIP.contains(&name.as_ref()) || name.starts_with("target-") {
                continue;
            }
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
                if out.len() >= INDEX_LIMIT {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Score `path` against a typed `query`: every query letter must appear in
/// order; runs, word starts and hits in the file name score higher.
pub fn fuzzy(path: &str, query: &str) -> Option<i32> {
    let hay: Vec<char> = path.chars().collect();
    let name_start = path.rfind('/').map_or(0, |i| path[..i].chars().count() + 1);
    let mut score = 0;
    let mut at = 0;
    let mut prev: Option<usize> = None;
    for q in query.chars().filter(|c| !c.is_whitespace()) {
        let q = q.to_ascii_lowercase();
        let i = (at..hay.len()).find(|i| hay[*i].to_ascii_lowercase() == q)?;
        score += 1;
        if prev == Some(i.wrapping_sub(1)) {
            score += 6;
        }
        if i == name_start || (i > 0 && matches!(hay[i - 1], '/' | '_' | '-' | '.')) {
            score += 8;
        }
        if i >= name_start {
            score += 3;
        }
        prev = Some(i);
        at = i + 1;
    }
    Some(score * 4 - hay.len() as i32 / 4)
}

pub fn header(ui: &mut Ui, pal: &Palette) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new("FOLDERS").size(11.5).color(pal.dim).strong());
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn fuzzy_prefers_file_name() {
        let a = fuzzy("crates/client/src/loot.rs", "loot").unwrap();
        let b = fuzzy("crates/client/src/lighting_tools/other.rs", "loot").unwrap();
        assert!(a > b);
        assert!(fuzzy("src/main.rs", "xyz").is_none());
        assert!(fuzzy("src/main.rs", "smr").is_some());
    }
}
