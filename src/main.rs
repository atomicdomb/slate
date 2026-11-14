//! Slate: a small native code editor.
//!
//!   slate [--theme NAME] [FOLDER | FILE]...
//!   slate --export-icons
//!
//! Each folder opens in its own window. This file owns the set of windows
//! and what they share; everything inside a window is in `workspace.rs`.

mod buffer;
mod editor;
mod highlight;
mod icons;
mod menu;
mod search;
mod theme;
mod tree;
mod workspace;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId};

use theme::Themes;
use workspace::{apply_visuals, Request, Shared, Workspace};

const APP_ID: &str = "slate";

struct App {
    sh: Shared,
    theme: usize,
    /// `windows[0]` is drawn in the window the OS gave us at startup; the
    /// rest get windows of their own. Never empty.
    windows: Vec<Workspace>,
    next_id: u64,
    allow_quit: bool,
}

/// The icon to hand the windowing system.
///
/// Inside a macOS `.app` this is deliberately empty: that tells eframe to
/// leave the Dock alone, so the bundle's own icon stays. (With no icon set
/// at all, eframe substitutes its default "e" logo.) Everywhere else the
/// icon is set from the same picture the bundle is built from.
fn app_icon() -> Arc<egui::IconData> {
    // Decoded once; every window shares it.
    static ICON: std::sync::OnceLock<Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| Arc::new(load_app_icon())).clone()
}

fn load_app_icon() -> egui::IconData {
    let bundled = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
    if bundled {
        return egui::IconData::default();
    }
    match image::load_from_memory(include_bytes!("../assets/AppIcon.png")) {
        Ok(picture) => {
            let small = picture.resize(256, 256, image::imageops::FilterType::Triangle).to_rgba8();
            egui::IconData { width: small.width(), height: small.height(), rgba: small.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn window_builder(title: &str) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(title)
        .with_icon(app_icon())
        .with_app_id(APP_ID)
        .with_inner_size([1500.0, 960.0])
        .with_min_inner_size([480.0, 320.0])
}

fn lines(text: Option<String>) -> Vec<PathBuf> {
    text.unwrap_or_default().lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()
}

fn joined(paths: impl Iterator<Item = PathBuf>) -> String {
    paths.map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n")
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, args: Vec<String>) -> Self {
        let settings = eframe::storage_dir(APP_ID);
        let themes_dir = settings.as_ref().map(|d| d.join("themes"));
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
                let p = PathBuf::from(a);
                paths.push(std::fs::canonicalize(&p).unwrap_or(p));
            }
        }
        let theme = themes
            .index_of(&theme_name)
            .or_else(|| themes.index_of(theme::DEFAULT_THEME))
            .unwrap_or(0);

        let sh = Shared {
            ss: Arc::new(ss),
            syntax_names: Arc::new(syntax_names),
            themes: Arc::new(themes),
            icons: Rc::new(RefCell::new(icons::Icons::load(&cc.egui_ctx, settings.as_deref()))),
            recent: Rc::new(RefCell::new(lines(stored("recent")))),
        };
        let mut app = Self { sh, theme, windows: Vec::new(), next_id: 0, allow_quit: false };
        app.add_window(None);
        apply_visuals(&cc.egui_ctx, app.windows[0].palette());

        if paths.is_empty() {
            // Nothing asked for: bring back the windows from last time.
            let mut last = lines(stored("folders"));
            last.extend(stored("folder").map(PathBuf::from)); // pre-multi-window setting
            paths = last.into_iter().filter(|p| p.is_dir()).collect();
            paths.dedup();
        }
        for p in paths {
            if !p.is_dir() {
                app.windows[0].open_path(&p, 0.0);
            } else if app.windows[0].folder().is_none() {
                app.windows[0].open_folder(p);
            } else {
                app.add_window(Some(p));
            }
        }
        app
    }

    fn add_window(&mut self, folder: Option<PathBuf>) {
        let mut window = Workspace::new(self.next_id, self.sh.clone(), self.theme);
        self.next_id += 1;
        if let Some(folder) = folder {
            window.open_folder(folder);
        }
        self.windows.push(window);
    }

    fn quit(&mut self, ctx: &egui::Context) {
        self.allow_quit = true;
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
    }

    fn close_window(&mut self, i: usize, ctx: &egui::Context) {
        if self.windows.len() == 1 {
            return self.quit(ctx);
        }
        self.windows.remove(i);
        if i == 0 {
            // The startup window cannot go away without ending the
            // program, so instead it takes over the next project: that
            // project's own window closes, and this one moves to where it
            // was.
            let heir = &mut self.windows[0];
            if let Some((position, size)) = heir.geometry {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::OuterPosition(position));
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::InnerSize(size));
            }
            heir.moved_to_new_window();
        }
    }

    fn handle(&mut self, window: u64, request: Request, ctx: &egui::Context, now: f64) {
        let Some(i) = self.windows.iter().position(|w| w.id == window) else { return };
        match request {
            Request::NewWindow(folder) => {
                // Opening a folder that already has a window brings that
                // window forward instead of making a twin.
                let existing = folder.as_deref().and_then(|f| self.windows.iter().position(|w| w.folder() == Some(f)));
                match existing {
                    Some(0) => ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus),
                    Some(j) => ctx.send_viewport_cmd_to(viewport_of(&self.windows[j]), ViewportCommand::Focus),
                    None => match (folder, self.windows.iter().position(|w| w.is_blank())) {
                        (Some(folder), Some(blank)) => self.windows[blank].open_folder(folder),
                        (folder, _) => self.add_window(folder),
                    },
                }
            }
            Request::CloseWindow => self.close_window(i, ctx),
            Request::Quit => match self.windows.iter().map(|w| w.unsaved()).sum() {
                0 => self.quit(ctx),
                unsaved => self.windows[i].ask_before_quit(unsaved),
            },
            Request::QuitNow { save } => {
                if !save || self.windows.iter_mut().all(|w| w.save_all(now)) {
                    self.quit(ctx);
                }
            }
            Request::Theme(theme) => {
                self.theme = theme;
                for w in &mut self.windows {
                    w.set_theme(theme);
                }
                apply_visuals(ctx, self.windows[0].palette());
            }
        }
    }
}

fn viewport_of(window: &Workspace) -> ViewportId {
    ViewportId::from_hash_of(("slate-window", window.id))
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let mut requests: Vec<(u64, Request)> = Vec::new();
        let mut out = Vec::new();

        // The close button of the startup window.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_quit {
            let alone = self.windows.len() == 1;
            let first = &mut self.windows[0];
            if !alone {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                first.request_close_window(&mut out);
            } else if first.unsaved() > 0 {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                first.ask_before_quit(first.unsaved());
            }
        }
        let first = &mut self.windows[0];
        first.ui(ctx, &mut out);
        requests.extend(out.drain(..).map(|r| (first.id, r)));

        for window in self.windows.iter_mut().skip(1) {
            let builder = window_builder(&window.title).with_inner_size([1300.0, 860.0]);
            ctx.show_viewport_immediate(viewport_of(window), builder, |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    window.request_close_window(&mut out);
                }
                window.ui(ctx, &mut out);
            });
            requests.extend(out.drain(..).map(|r| (window.id, r)));
        }

        for (window, request) in requests {
            self.handle(window, request, ctx, now);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("theme", self.sh.themes.list[self.theme].0.clone());
        storage.set_string("folders", joined(self.windows.iter().filter_map(|w| w.folder().map(PathBuf::from))));
        storage.set_string("recent", joined(self.sh.recent.borrow().iter().cloned()));
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
    let options = eframe::NativeOptions { viewport: window_builder("Slate"), ..Default::default() };
    eframe::run_native(APP_ID, options, Box::new(move |cc| Ok(Box::new(App::new(cc, args)))))
}
