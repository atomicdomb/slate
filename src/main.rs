//! Slate: a small native code editor.
//!
//!   slate [--theme NAME] [FOLDER | FILE]...
//!   slate --export-icons

mod buffer;
mod editor;
mod highlight;
mod icons;
mod search;
mod theme;
mod tree;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, CornerRadius, FontId, Frame, Key, KeyboardShortcut,
    Layout, Margin, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke, ViewportCommand,
};
use syntect::parsing::SyntaxSet;

use buffer::{Buffer, EditKind, Indent, Pos};
use editor::{Query, Reveal, View};
use highlight::Highlighter;
use theme::{Palette, Themes};

const APP_ID: &str = "slate";
const TAB_HEIGHT: f32 = 34.0;
const RESULT_ROW: f32 = 21.0;

struct Doc {
    path: Option<PathBuf>,
    name: String,
    buf: Buffer,
    hl: Highlighter,
    view: View,
}

#[derive(Default)]
struct Find {
    open: bool,
    /// Show the replace row too.
    replace: bool,
    query: Query,
    with: String,
    grab_focus: bool,
}

#[derive(Default)]
struct QuickOpen {
    query: String,
    /// Indices into `App::files`, best match first.
    hits: Vec<usize>,
    selected: usize,
}

enum Prompt {
    CloseTab(usize),
    Quit,
}

/// Things the UI asks for while it is being drawn; applied afterwards so
/// the drawing code never needs `&mut App`.
enum Action {
    Activate(usize),
    Close(usize),
    Open(PathBuf),
    Theme(usize),
    Syntax(String),
    Indent(Indent),
    FindNext { forward: bool, from_start: bool },
    Replace,
    ReplaceAll,
    CloseFind,
    /// Open a file and select columns `start..end` on `line`.
    OpenAt { path: PathBuf, line: usize, start: usize, end: usize },
    Search,
    CloseSearch,
    OpenFile,
    OpenFolder,
}

struct App {
    ss: SyntaxSet,
    syntax_names: Vec<String>,
    themes: Themes,
    theme: usize,
    pal: Palette,
    icons: icons::Icons,
    docs: Vec<Doc>,
    active: usize,
    root: Option<tree::Node>,
    files: Vec<String>,
    branch: Option<String>,
    sidebar: bool,
    find: Find,
    search: search::Search,
    quick: Option<QuickOpen>,
    goto: Option<String>,
    prompt: Option<Prompt>,
    focus_editor: bool,
    allow_quit: bool,
    /// A short notice in the status bar, and when it expires.
    notice: Option<(String, f64)>,
    title: String,
    untitled: usize,
}

fn apply_visuals(ctx: &egui::Context, pal: &Palette) {
    let mut v = if pal.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = pal.panel;
    v.window_fill = pal.panel;
    v.extreme_bg_color = pal.bg;
    v.selection.bg_fill = pal.selection;
    v.selection.stroke = Stroke::new(1.0_f32, pal.fg);
    v.widgets.noninteractive.fg_stroke.color = pal.fg.gamma_multiply(0.85);
    v.widgets.noninteractive.bg_stroke.color = pal.hover;
    v.window_stroke = Stroke::new(1.0_f32, pal.hover);
    ctx.set_visuals(v);
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, args: Vec<String>) -> Self {
        let themes_dir = eframe::storage_dir(APP_ID).map(|d| d.join("themes"));
        if let Some(dir) = &themes_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        let themes = Themes::load(themes_dir.as_deref());
        let ss = two_face::syntax::extra_newlines();
        let mut syntax_names: Vec<String> = ss.syntaxes().iter().map(|s| s.name.clone()).collect();
        syntax_names.sort_by_key(|n| n.to_lowercase());

        let stored = |key: &str| cc.storage.and_then(|s| s.get_string(key));
        let mut theme_name = stored("theme").unwrap_or_else(|| theme::DEFAULT_THEME.to_string());
        let mut paths = Vec::new();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            if a == "--theme" {
                theme_name = args.next().unwrap_or(theme_name);
            } else {
                paths.push(PathBuf::from(a));
            }
        }
        let theme = themes
            .index_of(&theme_name)
            .or_else(|| themes.index_of(theme::DEFAULT_THEME))
            .unwrap_or(0);
        let pal = Palette::from_theme(&themes.list[theme].1);
        apply_visuals(&cc.egui_ctx, &pal);
        let icons = icons::Icons::load(&cc.egui_ctx, eframe::storage_dir(APP_ID).as_deref());

        let mut app = Self {
            ss,
            syntax_names,
            themes,
            theme,
            pal,
            icons,
            docs: Vec::new(),
            active: 0,
            root: None,
            files: Vec::new(),
            branch: None,
            sidebar: true,
            find: Find::default(),
            search: search::Search::default(),
            quick: None,
            goto: None,
            prompt: None,
            focus_editor: true,
            allow_quit: false,
            notice: None,
            title: String::new(),
            untitled: 0,
        };
        if paths.is_empty() {
            if let Some(folder) = stored("folder").map(PathBuf::from).filter(|p| p.is_dir()) {
                app.open_folder(folder);
            }
        }
        for p in paths {
            let p = std::fs::canonicalize(&p).unwrap_or(p);
            if p.is_dir() {
                app.open_folder(p);
            } else {
                app.open_path(&p, 0.0);
            }
        }
        app
    }

    fn doc(&mut self) -> Option<&mut Doc> {
        self.docs.get_mut(self.active)
    }

    fn say(&mut self, text: impl Into<String>, now: f64) {
        self.notice = Some((text.into(), now + 4.0));
    }

    fn open_folder(&mut self, path: PathBuf) {
        self.files = tree::index(&path);
        self.branch = std::fs::read_to_string(path.join(".git/HEAD")).ok().map(|head| {
            let head = head.trim();
            match head.strip_prefix("ref: refs/heads/") {
                Some(branch) => branch.to_string(),
                None => head.chars().take(7).collect(),
            }
        });
        self.root = Some(tree::Node::root(path));
        self.sidebar = true;
    }

    fn open_path(&mut self, path: &Path, now: f64) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path)) {
            self.active = i;
            self.focus_editor = true;
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return self.say(format!("Could not open {}: {e}", path.display()), now),
        };
        if bytes.iter().take(8000).any(|b| *b == 0) {
            return self.say(format!("{} is a binary file", path.display()), now);
        }
        let buf = Buffer::from_text(&String::from_utf8_lossy(&bytes));
        let syntax = highlight::syntax_for(&self.ss, Some(path), &buf.lines[0]);
        let hl = Highlighter::new(&syntax.name, buf.lines.len());
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().to_string());
        // An untouched empty "untitled" tab is replaced rather than kept.
        if self.docs.len() == 1 && self.docs[0].path.is_none() && !self.docs[0].buf.dirty {
            self.docs.clear();
        }
        self.docs.push(Doc { path: Some(path.to_path_buf()), name, buf, hl, view: View::default() });
        self.active = self.docs.len() - 1;
        self.focus_editor = true;
    }

    fn new_doc(&mut self) {
        self.untitled += 1;
        let buf = Buffer::from_text("");
        let hl = Highlighter::new("Plain Text", 1);
        let name = if self.untitled == 1 { "untitled".into() } else { format!("untitled {}", self.untitled) };
        self.docs.push(Doc { path: None, name, buf, hl, view: View::default() });
        self.active = self.docs.len() - 1;
        self.focus_editor = true;
    }

    /// Save tab `i`, asking for a name if it has none. False if cancelled or failed.
    fn save(&mut self, i: usize, save_as: bool, now: f64) -> bool {
        let Some(doc) = self.docs.get(i) else { return false };
        let path = match (&doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let mut dialog = rfd::FileDialog::new().set_file_name(&doc.name);
                let dir = doc.path.as_ref().and_then(|p| p.parent()).or(self.root.as_ref().map(|r| r.path.as_path()));
                if let Some(dir) = dir {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        if let Err(e) = std::fs::write(&path, self.docs[i].buf.text()) {
            self.say(format!("Could not save {}: {e}", path.display()), now);
            return false;
        }
        let renamed = self.docs[i].path.as_deref() != Some(path.as_path());
        let doc = &mut self.docs[i];
        doc.buf.dirty = false;
        if renamed {
            doc.name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let syntax = highlight::syntax_for(&self.ss, Some(&path), &doc.buf.lines[0]);
            doc.hl.set_syntax(&syntax.name);
            doc.path = Some(path);
            if let Some(root) = &mut self.root {
                root.refresh();
                self.files = tree::index(&root.path);
            }
        }
        true
    }

    fn close(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        self.docs.remove(i);
        if self.active > i || self.active >= self.docs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.focus_editor = true;
    }

    fn request_close(&mut self, i: usize) {
        match self.docs.get(i) {
            Some(d) if d.buf.dirty => self.prompt = Some(Prompt::CloseTab(i)),
            Some(_) => self.close(i),
            None => {}
        }
    }

    fn set_theme(&mut self, i: usize, ctx: &egui::Context) {
        self.theme = i;
        self.pal = Palette::from_theme(&self.themes.list[i].1);
        apply_visuals(ctx, &self.pal);
        for d in &mut self.docs {
            d.hl.invalidate();
        }
    }

    /// Select the next (or previous) match, wrapping around the file.
    /// `from_start` searches from the start of the selection, so the match
    /// under the caret grows as the query is typed.
    fn find_next(&mut self, forward: bool, from_start: bool, now: f64) {
        let query = self.find.query.clone();
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if query.text.is_empty() {
            return;
        }
        let buf = &mut doc.buf;
        let (lo, hi) = buf.selection().unwrap_or((buf.cursor, buf.cursor));
        let n = buf.lines.len();
        let mut found = None;
        // Visit every line once, plus the starting line a second time for
        // the part on the other side of the caret.
        for step in 0..=n {
            let line = if forward { (lo.line + step) % n } else { (hi.line + n - step % n) % n };
            let hits = query.matches(&buf.lines[line]);
            let pick = if forward {
                let after = if from_start { lo.col } else { hi.col };
                hits.iter().find(|(a, _)| step > 0 || *a >= after)
            } else {
                hits.iter().rev().find(|(_, b)| step > 0 || *b <= lo.col)
            };
            if let Some((a, b)) = pick {
                found = Some((Pos::new(line, *a), Pos::new(line, *b)));
                break;
            }
        }
        match found {
            Some((a, b)) => {
                buf.move_to(a, false);
                buf.move_to(b, true);
                doc.view.reveal = Reveal::Center;
            }
            None => self.say(format!("No match for “{}”", query.text), now),
        }
    }

    /// Replace the match under the selection, if there is one, and move
    /// on to the next.
    fn replace_one(&mut self, now: f64) {
        let (query, with) = (self.find.query.clone(), self.find.with.clone());
        if let Some(doc) = self.docs.get_mut(self.active) {
            if let Some((a, b)) = doc.buf.selection() {
                if a.line == b.line && query.matches(&doc.buf.lines[a.line]).contains(&(a.col, b.col)) {
                    doc.buf.insert(&with, EditKind::Other);
                }
            }
        }
        self.find_next(true, false, now);
    }

    /// Replace every match in the active tab, as a single undo step.
    fn replace_all(&mut self, now: f64) {
        let (query, with) = (self.find.query.clone(), self.find.with.clone());
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let ranges: Vec<(Pos, Pos)> = doc
            .buf
            .lines
            .iter()
            .enumerate()
            .flat_map(|(line, text)| {
                query.matches(text).into_iter().map(move |(a, b)| (Pos::new(line, a), Pos::new(line, b)))
            })
            .collect();
        if ranges.is_empty() {
            return self.say(format!("No match for “{}”", query.text), now);
        }
        doc.buf.replace_ranges(&ranges, &with);
        doc.view.reveal = Reveal::Nearest;
        let n = ranges.len();
        self.say(format!("Replaced {n} occurrence{}", if n == 1 { "" } else { "s" }), now);
    }

    /// The selection, if it is the sort of thing one searches for.
    fn selection_for_search(&self) -> Option<String> {
        let doc = self.docs.get(self.active)?;
        doc.buf.selected_text().filter(|s| !s.contains('\n'))
    }

    fn open_find(&mut self, replace: bool) {
        if let Some(sel) = self.selection_for_search() {
            self.find.query.text = sel;
        }
        self.find.open = true;
        self.find.replace = replace;
        self.find.grab_focus = true;
    }

    fn open_search(&mut self, now: f64) {
        if self.root.is_none() {
            return self.say("Open a folder to search in files", now);
        }
        if let Some(sel) = self.selection_for_search() {
            self.search.query.text = sel;
        }
        self.search.open = true;
        self.search.grab_focus = true;
    }

    fn start_search(&mut self, ctx: &egui::Context) {
        let Some(root) = &mut self.root else { return };
        // Re-read the file list so files created since are included.
        self.files = tree::index(&root.path);
        let unsaved: HashMap<PathBuf, String> = self
            .docs
            .iter()
            .filter(|d| d.buf.dirty)
            .filter_map(|d| Some((d.path.clone()?, d.buf.lines.join("\n"))))
            .collect();
        self.search.start(&root.path, self.files.clone(), unsaved, ctx);
    }

    fn open_at(&mut self, path: &Path, line: usize, start: usize, end: usize, now: f64) {
        self.open_path(path, now);
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        if doc.path.as_deref() != Some(path) {
            return;
        }
        doc.buf.move_to(Pos::new(line, start), false);
        doc.buf.move_to(Pos::new(line, end), true);
        doc.view.reveal = Reveal::Center;
    }

    fn pick_file(&mut self, now: f64) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(root) = &self.root {
            dialog = dialog.set_directory(&root.path);
        }
        for p in dialog.pick_files().unwrap_or_default() {
            self.open_path(&p, now);
        }
    }

    fn pick_folder(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(p);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, now: f64) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let hit = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        // Shifted variants first: a plain pattern also matches with shift held.
        if hit(cmd_shift, Key::O) {
            self.pick_folder();
        } else if hit(cmd, Key::O) {
            self.pick_file(now);
        }
        if hit(cmd_shift, Key::S) {
            self.save(self.active, true, now);
        } else if hit(cmd, Key::S) {
            self.save(self.active, false, now);
        }
        if hit(cmd, Key::N) {
            self.new_doc();
        }
        if hit(cmd, Key::W) {
            self.request_close(self.active);
        }
        if hit(cmd, Key::B) {
            self.sidebar = !self.sidebar;
        }
        if hit(cmd, Key::P) && self.root.is_some() {
            let mut q = QuickOpen::default();
            q.hits = (0..self.files.len().min(200)).collect();
            self.quick = Some(q);
        }
        let mac = cfg!(target_os = "macos");
        let replace_keys = if mac { hit(cmd | Modifiers::ALT, Key::F) } else { hit(cmd, Key::H) };
        if hit(cmd_shift, Key::F) {
            self.open_search(now);
        } else if replace_keys && !self.docs.is_empty() {
            self.open_find(true);
        } else if hit(cmd, Key::F) && !self.docs.is_empty() {
            self.open_find(false);
        }
        if hit(Modifiers::SHIFT, Key::F3) || (mac && hit(cmd_shift, Key::G)) {
            self.find_next(false, false, now);
        } else if hit(Modifiers::NONE, Key::F3) || (mac && hit(cmd, Key::G)) {
            self.find_next(true, false, now);
        }
        if hit(Modifiers::CTRL, Key::G) && !self.docs.is_empty() {
            self.goto = Some(String::new());
        }
        if !self.docs.is_empty() {
            let n = self.docs.len();
            if hit(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
                self.active = (self.active + n - 1) % n;
                self.focus_editor = true;
            } else if hit(Modifiers::CTRL, Key::Tab) {
                self.active = (self.active + 1) % n;
                self.focus_editor = true;
            }
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context, now: f64, actions: &mut Vec<Action>) {
        let pal = self.pal.clone();
        let frame = Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(12, 0));
        egui::TopBottomPanel::bottom("status").exact_height(26.0).frame(frame).show(ctx, |ui| {
            ui.style_mut().override_font_id = Some(FontId::proportional(12.5));
            ui.horizontal_centered(|ui| {
                if let Some(doc) = self.docs.get(self.active) {
                    let c = doc.buf.cursor;
                    let mut text = format!("Line {}, Column {}", c.line + 1, c.col + 1);
                    if let Some(sel) = doc.buf.selected_text() {
                        let lines = sel.matches('\n').count() + 1;
                        if lines > 1 {
                            text = format!("{lines} lines, {} characters selected", sel.chars().count());
                        } else {
                            text = format!("{} characters selected", sel.chars().count());
                        }
                    }
                    ui.label(text);
                }
                if let Some((notice, until)) = &self.notice {
                    if now < *until {
                        ui.add_space(16.0);
                        ui.label(RichText::new(notice).color(pal.find));
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    if let Some(doc) = self.docs.get(self.active) {
                        egui::ComboBox::from_id_salt("syntax")
                            .selected_text(&doc.hl.syntax)
                            .height(420.0)
                            .show_ui(ui, |ui| {
                                for name in &self.syntax_names {
                                    if ui.selectable_label(*name == doc.hl.syntax, name).clicked() {
                                        actions.push(Action::Syntax(name.clone()));
                                    }
                                }
                            });
                        let label = match doc.buf.indent {
                            Indent::Tabs => "Tabs".to_string(),
                            Indent::Spaces(n) => format!("Spaces: {n}"),
                        };
                        ui.menu_button(label, |ui| {
                            for choice in [Indent::Spaces(2), Indent::Spaces(4), Indent::Spaces(8), Indent::Tabs] {
                                let text = match choice {
                                    Indent::Tabs => "Tabs".to_string(),
                                    Indent::Spaces(n) => format!("Spaces: {n}"),
                                };
                                if ui.selectable_label(choice == doc.buf.indent, text).clicked() {
                                    actions.push(Action::Indent(choice));
                                    ui.close();
                                }
                            }
                        });
                    }
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(&self.themes.list[self.theme].0)
                        .height(520.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in self.themes.list.iter().enumerate() {
                                if ui.selectable_label(i == self.theme, name).clicked() {
                                    actions.push(Action::Theme(i));
                                }
                            }
                        });
                    if let Some(branch) = &self.branch {
                        ui.label(RichText::new(format!("git: {branch}")).color(pal.dim));
                    }
                });
            });
        });
    }

    fn side_bar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let pal = self.pal.clone();
        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
        let Some(root) = &mut self.root else { return };
        egui::SidePanel::left("folders")
            .default_width(250.0)
            .width_range(140.0..=600.0)
            .frame(Frame::new().fill(pal.panel))
            .show(ctx, |ui| {
                tree::header(ui, &pal);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if let Some(path) = root.show(ui, 0, active.as_deref(), &pal, &self.icons) {
                        actions.push(Action::Open(path));
                    }
                    ui.add_space(12.0);
                });
            });
    }

    fn tab_bar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let pal = &self.pal;
        egui::TopBottomPanel::top("tabs")
            .exact_height(TAB_HEIGHT)
            .frame(Frame::new().fill(pal.strip))
            .show_separator_line(false)
            .show(ctx, |ui| {
                let scroll = ScrollArea::horizontal()
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
                scroll.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for (i, doc) in self.docs.iter().enumerate() {
                            let active = i == self.active;
                            let color = if active { pal.fg } else { pal.dim };
                            let text = ui.painter().layout_no_wrap(doc.name.clone(), FontId::proportional(13.0), color);
                            let width = (text.size().x + 58.0).max(130.0);
                            let (rect, resp) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
                            let p = ui.painter();
                            if active {
                                let body = Rect::from_min_max(rect.min + vec2(0.0, 4.0), rect.max);
                                p.rect_filled(body, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, pal.bg);
                            } else if resp.hovered() {
                                let body = Rect::from_min_max(rect.min + vec2(0.0, 4.0), rect.max);
                                p.rect_filled(body, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, pal.strip.lerp_to_gamma(pal.bg, 0.5));
                            }
                            let y = rect.center().y + 2.0;
                            p.galley(pos2(rect.left() + 16.0, y - text.size().y / 2.0), text, color);

                            let x_center = pos2(rect.right() - 17.0, y);
                            let close = Rect::from_center_size(x_center, vec2(20.0, 20.0));
                            let over_close = resp.hover_pos().is_some_and(|p| close.contains(p));
                            if doc.buf.dirty && !over_close {
                                p.circle_filled(x_center, 3.5, color);
                            } else if active || resp.hovered() {
                                if over_close {
                                    p.rect_filled(close, 4.0, pal.hover);
                                }
                                let d = 3.5;
                                let stroke = Stroke::new(1.3_f32, if over_close { pal.fg } else { pal.dim });
                                p.line_segment([x_center + vec2(-d, -d), x_center + vec2(d, d)], stroke);
                                p.line_segment([x_center + vec2(-d, d), x_center + vec2(d, -d)], stroke);
                            }
                            if resp.middle_clicked() || (resp.clicked() && over_close) {
                                actions.push(Action::Close(i));
                            } else if resp.clicked() {
                                actions.push(Action::Activate(i));
                            }
                            if let Some(path) = &doc.path {
                                resp.on_hover_text(path.to_string_lossy());
                            }
                        }
                    });
                });
            });
    }

    fn find_bar(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.find.open {
            return;
        }
        let pal = self.pal.clone();
        let count: usize = self
            .docs
            .get(self.active)
            .map_or(0, |d| d.buf.lines.iter().map(|l| self.find.query.matches(l).len()).sum());
        let frame = Frame::new().fill(pal.panel).inner_margin(Margin::symmetric(10, 6));
        let find = &mut self.find;
        egui::TopBottomPanel::bottom("find").frame(frame).show_separator_line(false).show(ctx, |ui| {
            let enter = |ui: &egui::Ui, resp: &egui::Response| resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let row = 22.0;
            ui.spacing_mut().interact_size.y = row;
            let field_width = (ui.available_width() - 470.0).max(120.0);
            fn field<'a>(text: &'a mut String, hint: &'static str, width: f32, height: f32) -> egui::TextEdit<'a> {
                egui::TextEdit::singleline(text)
                    .font(FontId::monospace(editor::FONT_SIZE))
                    .hint_text(hint)
                    .desired_width(width)
                    .min_size(vec2(0.0, height))
                    .vertical_align(Align::Center)
            }
            // Columns rather than rows, so Tab goes from the find field
            // straight to the replace field.
            ui.horizontal_top(|ui| {
                query_toggles(ui, &mut find.query);
                ui.vertical(|ui| {
                    let resp = ui.add(field(&mut find.query.text, "Find", field_width, row));
                    if std::mem::take(&mut find.grab_focus) {
                        resp.request_focus();
                    }
                    if resp.changed() {
                        actions.push(Action::FindNext { forward: true, from_start: true });
                    }
                    if enter(ui, &resp) {
                        let forward = !ui.input(|i| i.modifiers.shift);
                        actions.push(Action::FindNext { forward, from_start: false });
                        resp.request_focus();
                    }
                    if find.replace {
                        let resp = ui.add(field(&mut find.with, "Replace with", field_width, row));
                        if enter(ui, &resp) {
                            actions.push(Action::Replace);
                            resp.request_focus();
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Find").clicked() {
                            actions.push(Action::FindNext { forward: true, from_start: false });
                        }
                        if ui.button("Find Prev").clicked() {
                            actions.push(Action::FindNext { forward: false, from_start: false });
                        }
                        let summary = match count {
                            _ if find.query.text.is_empty() => String::new(),
                            0 => "No matches".to_string(),
                            1 => "1 match".to_string(),
                            n => format!("{n} matches"),
                        };
                        ui.label(RichText::new(summary).color(pal.dim));
                    });
                    if find.replace {
                        ui.horizontal(|ui| {
                            if ui.button("Replace").clicked() {
                                actions.push(Action::Replace);
                            }
                            if ui.button("Replace All").clicked() {
                                actions.push(Action::ReplaceAll);
                            }
                        });
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.button("×").on_hover_text("Close (Esc)").clicked() {
                        actions.push(Action::CloseFind);
                    }
                    if ui.selectable_label(find.replace, "Replace…").clicked() {
                        find.replace = !find.replace;
                    }
                });
            });
        });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            actions.push(Action::CloseFind);
        }
    }

    fn search_panel(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.search.open {
            return;
        }
        self.search.poll();
        let Some(root) = self.root.as_ref().map(|r| r.path.clone()) else { return };
        let pal = self.pal.clone();
        let icons = &self.icons;
        let search = &mut self.search;
        let frame = Frame::new().fill(pal.panel).inner_margin(Margin { left: 0, right: 0, top: 6, bottom: 0 });
        egui::TopBottomPanel::bottom("search")
            .resizable(true)
            .default_height(290.0)
            .height_range(110.0..=700.0)
            .frame(frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    query_toggles(ui, &mut search.query);
                    let field = egui::TextEdit::singleline(&mut search.query.text)
                        .font(FontId::monospace(editor::FONT_SIZE))
                        .hint_text("Find in files")
                        .desired_width((ui.available_width() - 470.0).max(120.0));
                    let resp = ui.add(field);
                    if std::mem::take(&mut search.grab_focus) {
                        resp.request_focus();
                    }
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if enter || ui.button("Find").clicked() {
                        actions.push(Action::Search);
                    }
                    ui.label(RichText::new(search.summary()).color(pal.dim));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(10.0);
                        if ui.button("×").on_hover_text("Close (Esc)").clicked() {
                            actions.push(Action::CloseSearch);
                        }
                    });
                });
                ui.add_space(6.0);

                // One row per file header and per matching line.
                let rows: Vec<(usize, Option<usize>)> = search
                    .results
                    .iter()
                    .enumerate()
                    .flat_map(|(f, file)| {
                        let hits = if file.collapsed { 0 } else { file.hits.len() };
                        std::iter::once((f, None)).chain((0..hits).map(move |h| (f, Some(h))))
                    })
                    .collect();
                let mono = FontId::monospace(12.5);
                let mut toggle = None;
                ui.spacing_mut().item_spacing.y = 0.0;
                ScrollArea::vertical().auto_shrink(false).show_rows(ui, RESULT_ROW, rows.len(), |ui, range| {
                    for &(f, hit) in &rows[range] {
                        let file = &search.results[f];
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), RESULT_ROW), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 0.0, pal.hover);
                        }
                        let p = ui.painter().with_clip_rect(rect);
                        let mid = rect.center().y;
                        match hit {
                            None => {
                                let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                                let at = Rect::from_min_size(pos2(rect.left() + 12.0, mid - 8.0), vec2(16.0, 16.0));
                                icons.paint(&p, icons::Icon::for_file(name), at);
                                let end = p.text(pos2(at.right() + 7.0, mid), Align2::LEFT_CENTER, &file.path, FontId::proportional(13.0), pal.fg);
                                p.text(pos2(end.right() + 10.0, mid), Align2::LEFT_CENTER, file.matches.to_string(), FontId::proportional(12.0), pal.dim);
                                if resp.clicked() {
                                    toggle = Some(f);
                                }
                            }
                            Some(h) => {
                                let hit = &file.hits[h];
                                let number_right = rect.left() + 82.0;
                                p.text(pos2(number_right, mid), Align2::RIGHT_CENTER, (hit.line + 1).to_string(), mono.clone(), pal.gutter_fg);
                                let galley = ui.fonts(|fonts| fonts.layout_job(snippet(&hit.text, &search.ran, &mono, &pal)));
                                p.galley(pos2(number_right + 14.0, mid - galley.size().y / 2.0), galley, pal.fg);
                                if resp.clicked() {
                                    actions.push(Action::OpenAt { path: root.join(&file.path), line: hit.line, start: hit.start, end: hit.end });
                                }
                            }
                        }
                    }
                });
                if let Some(f) = toggle {
                    search.results[f].collapsed ^= true;
                }
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) && !self.find.open {
            actions.push(Action::CloseSearch);
        }
    }

    fn central(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let overlay = self.quick.is_some() || self.goto.is_some() || self.prompt.is_some();
        let grab_focus = std::mem::take(&mut self.focus_editor) && !overlay;
        egui::CentralPanel::default().frame(Frame::new().fill(self.pal.bg)).show(ctx, |ui| {
            let Some(doc) = self.docs.get_mut(self.active) else {
                return welcome(ui, &self.pal, actions);
            };
            let find = (self.find.open && !self.find.query.text.is_empty()).then_some(&self.find.query);
            let env = editor::Env {
                pal: &self.pal,
                ss: &self.ss,
                theme: &self.themes.list[self.theme].1,
                find,
                grab_focus,
                enabled: !overlay,
            };
            editor::show(ui, &mut doc.buf, &mut doc.hl, &mut doc.view, &env);
        });
    }

    fn quick_open(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(q) = &mut self.quick else { return };
        let Some(root) = &self.root else { return };
        let pal = &self.pal;
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let (up, down, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        let shown = q.hits.len().min(14);
        if down && shown > 0 {
            q.selected = (q.selected + 1) % shown;
        }
        if up && shown > 0 {
            q.selected = (q.selected + shown - 1) % shown;
        }
        let mut chosen = enter.then_some(q.selected);

        egui::Window::new("quick-open")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 56.0))
            .fixed_size(vec2(560.0, 0.0))
            .show(ctx, |ui| {
                let field = egui::TextEdit::singleline(&mut q.query)
                    .hint_text("Go to file…")
                    .font(FontId::proportional(15.0))
                    .desired_width(f32::INFINITY);
                let resp = ui.add(field);
                resp.request_focus();
                if resp.changed() {
                    let mut scored: Vec<(i32, usize)> = self
                        .files
                        .iter()
                        .enumerate()
                        .filter_map(|(i, f)| tree::fuzzy(f, &q.query).map(|s| (s, i)))
                        .collect();
                    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                    q.hits = scored.into_iter().take(200).map(|(_, i)| i).collect();
                    q.selected = 0;
                }
                ui.add_space(4.0);
                for (row, &file) in q.hits.iter().take(14).enumerate() {
                    let path = &self.files[file];
                    let (dir, name) = match path.rfind('/') {
                        Some(i) => (&path[..i], &path[i + 1..]),
                        None => ("", path.as_str()),
                    };
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    if row == q.selected || resp.hovered() {
                        ui.painter().rect_filled(rect, 4.0, if row == q.selected { pal.selection } else { pal.hover });
                    }
                    let p = ui.painter().with_clip_rect(rect);
                    let end = p.text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, name, FontId::proportional(13.5), pal.fg);
                    p.text(pos2(end.right() + 10.0, rect.center().y), Align2::LEFT_CENTER, dir, FontId::proportional(12.0), pal.dim);
                    if resp.clicked() {
                        chosen = Some(row);
                    }
                }
                if q.hits.is_empty() {
                    ui.label(RichText::new("No matching files").color(pal.dim));
                }
            });

        if let Some(file) = chosen.and_then(|row| q.hits.get(row)) {
            actions.push(Action::Open(root.path.join(&self.files[*file])));
            close = true;
        }
        if close {
            self.quick = None;
            self.focus_editor = true;
        }
    }

    fn goto_line(&mut self, ctx: &egui::Context) {
        let Some(text) = &mut self.goto else { return };
        let mut done = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut target = None;
        egui::Window::new("goto")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 56.0))
            .show(ctx, |ui| {
                let field = egui::TextEdit::singleline(text)
                    .hint_text("Line, or line:column")
                    .font(FontId::proportional(15.0))
                    .desired_width(260.0);
                let resp = ui.add(field);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let mut parts = text.split(':').map(|p| p.trim().parse::<usize>().ok());
                    if let Some(Some(line)) = parts.next() {
                        let col = parts.next().flatten().unwrap_or(1);
                        target = Some(Pos::new(line.saturating_sub(1), col.saturating_sub(1)));
                    }
                    done = true;
                } else {
                    resp.request_focus();
                }
            });
        if let (Some(pos), Some(doc)) = (target, self.docs.get_mut(self.active)) {
            doc.buf.move_to(pos, false);
            doc.view.reveal = Reveal::Center;
        }
        if done {
            self.goto = None;
            self.focus_editor = true;
        }
    }

    fn prompts(&mut self, ctx: &egui::Context, now: f64) {
        let Some(prompt) = &self.prompt else { return };
        let (message, tab) = match prompt {
            Prompt::CloseTab(i) => match self.docs.get(*i) {
                Some(d) => (format!("Save changes to “{}” before closing?", d.name), Some(*i)),
                None => {
                    self.prompt = None;
                    return;
                }
            },
            Prompt::Quit => {
                let n = self.docs.iter().filter(|d| d.buf.dirty).count();
                let what = if n == 1 { "1 file has".to_string() } else { format!("{n} files have") };
                (format!("{what} unsaved changes. Save before quitting?"), None)
            }
        };
        // 0 = save, 1 = discard, 2 = cancel
        let mut choice = ctx.input(|i| i.key_pressed(Key::Escape)).then_some(2);
        egui::Window::new("unsaved")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(message).size(14.5));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    for (i, label) in ["Save", "Don’t Save", "Cancel"].into_iter().enumerate() {
                        if ui.button(label).clicked() {
                            choice = Some(i);
                        }
                    }
                });
                ui.add_space(2.0);
            });
        let Some(choice) = choice else { return };
        self.prompt = None;
        match (choice, tab) {
            (2, _) => {}
            (0, Some(i)) => {
                if self.save(i, false, now) {
                    self.close(i);
                }
            }
            (_, Some(i)) => self.close(i),
            (save, None) => {
                let dirty: Vec<usize> = (0..self.docs.len()).filter(|i| self.docs[*i].buf.dirty).collect();
                if save == 0 && !dirty.into_iter().all(|i| self.save(i, false, now)) {
                    return;
                }
                self.allow_quit = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn apply(&mut self, actions: Vec<Action>, ctx: &egui::Context, now: f64) {
        for action in actions {
            match action {
                Action::Activate(i) => {
                    self.active = i;
                    self.focus_editor = true;
                }
                Action::Close(i) => self.request_close(i),
                Action::Open(path) => self.open_path(&path, now),
                Action::Theme(i) => self.set_theme(i, ctx),
                Action::Syntax(name) => {
                    if let Some(doc) = self.doc() {
                        doc.hl.set_syntax(&name);
                    }
                }
                Action::Indent(indent) => {
                    if let Some(doc) = self.doc() {
                        doc.buf.indent = indent;
                    }
                }
                Action::FindNext { forward, from_start } => self.find_next(forward, from_start, now),
                Action::Replace => self.replace_one(now),
                Action::ReplaceAll => self.replace_all(now),
                Action::OpenAt { path, line, start, end } => self.open_at(&path, line, start, end, now),
                Action::Search => self.start_search(ctx),
                Action::CloseSearch => {
                    self.search.open = false;
                    self.search.stop();
                    self.focus_editor = true;
                }
                Action::CloseFind => {
                    self.find.open = false;
                    self.focus_editor = true;
                }
                Action::OpenFile => self.pick_file(now),
                Action::OpenFolder => self.pick_folder(),
            }
        }
    }
}

/// The "match case" and "whole word" switches shared by both find bars.
fn query_toggles(ui: &mut egui::Ui, query: &mut Query) {
    if ui.selectable_label(query.case, "Aa").on_hover_text("Match case").clicked() {
        query.case ^= true;
    }
    if ui.selectable_label(query.word, "W").on_hover_text("Whole word").clicked() {
        query.word ^= true;
    }
}

/// A result line with its matches picked out.
fn snippet(text: &str, query: &Query, font: &FontId, pal: &Palette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let chars: Vec<char> = text.chars().collect();
    let piece = |range: std::ops::Range<usize>| chars[range].iter().collect::<String>();
    let plain = egui::TextFormat { font_id: font.clone(), color: pal.fg.gamma_multiply(0.85), ..Default::default() };
    let marked = egui::TextFormat {
        font_id: font.clone(),
        color: pal.fg,
        background: pal.find.gamma_multiply(0.3),
        ..Default::default()
    };
    let mut at = 0;
    for (a, b) in query.matches(text) {
        job.append(&piece(at..a), 0.0, plain.clone());
        job.append(&piece(a..b), 0.0, marked.clone());
        at = b;
    }
    job.append(&piece(at..chars.len()), 0.0, plain);
    job
}

fn welcome(ui: &mut egui::Ui, pal: &Palette, actions: &mut Vec<Action>) {
    let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        ui.label(RichText::new("Slate").size(30.0).color(pal.dim));
        ui.add_space(14.0);
        if ui.button("Open Folder…").clicked() {
            actions.push(Action::OpenFolder);
        }
        ui.add_space(4.0);
        if ui.button("Open File…").clicked() {
            actions.push(Action::OpenFile);
        }
        ui.add_space(18.0);
        for (keys, what) in [("P", "go to file"), ("F", "find"), ("S", "save"), ("B", "toggle sidebar")] {
            ui.label(RichText::new(format!("{cmd}{keys}   {what}")).color(pal.dim));
        }
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);

        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_quit
            && self.docs.iter().any(|d| d.buf.dirty)
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.prompt = Some(Prompt::Quit);
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for path in dropped {
            if path.is_dir() {
                self.open_folder(path);
            } else {
                self.open_path(&path, now);
            }
        }
        // Coming back from another app: pick up files created meanwhile.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            if let Some(root) = &mut self.root {
                root.refresh();
            }
            if let Some(problem) = self.icons.reload(ctx) {
                self.say(problem, now);
            }
        }
        if self.prompt.is_none() {
            self.shortcuts(ctx, now);
        }

        let mut actions = Vec::new();
        self.status_bar(ctx, now, &mut actions);
        if self.sidebar {
            self.side_bar(ctx, &mut actions);
        }
        if !self.docs.is_empty() {
            self.tab_bar(ctx, &mut actions);
        }
        self.search_panel(ctx, &mut actions);
        self.find_bar(ctx, &mut actions);
        self.central(ctx, &mut actions);
        self.quick_open(ctx, &mut actions);
        self.goto_line(ctx);
        self.prompts(ctx, now);
        self.apply(actions, ctx, now);

        let title = match (self.docs.get(self.active), &self.root) {
            (Some(d), Some(r)) => format!("{} — {}", d.name, r.path.file_name().unwrap_or_default().to_string_lossy()),
            (Some(d), None) => d.name.clone(),
            (None, _) => "Slate".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", self.themes.list[self.theme].0.clone());
        if let Some(root) = &self.root {
            storage.set_string("folder", root.path.to_string_lossy().to_string());
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--export-icons") {
        let result = eframe::storage_dir(APP_ID)
            .ok_or_else(|| "no settings folder on this system".to_string())
            .and_then(|dir| icons::export(&dir));
        match result {
            Ok(path) => println!("Wrote {}\nEdit it; Slate reloads it when its window regains focus.", path.display()),
            Err(e) => {
                eprintln!("slate: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Slate")
            .with_app_id(APP_ID)
            .with_inner_size([1500.0, 960.0])
            .with_min_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(APP_ID, options, Box::new(move |cc| Ok(Box::new(App::new(cc, args)))))
}
