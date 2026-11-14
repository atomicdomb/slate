//! Commands, their keyboard shortcuts, and the menu bar.
//!
//! Every action that can be reached from a menu or a shortcut is a
//! [`Command`]. The menu bar and the key handling both read the same
//! shortcut table, so the two can never disagree.

use std::path::PathBuf;

use eframe::egui::{self, Button, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers};

use crate::theme::Palette;

#[derive(Clone, PartialEq, Debug)]
pub enum Command {
    NewFile,
    NewWindow,
    OpenFile,
    OpenFolder,
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    CloseTab,
    CloseWindow,
    Quit,
    Find,
    Replace,
    FindInFiles,
    FindNext,
    FindPrevious,
    GoToLine,
    GoToFile,
    ToggleSidebar,
    NextTab,
    PreviousTab,
}

/// Commands with a shortcut, in the order the keys must be tested:
/// shifted variants first, because a plain pattern also matches with
/// Shift held.
pub const KEYED: &[Command] = &[
    Command::OpenFolder,
    Command::OpenFile,
    Command::SaveAs,
    Command::Save,
    Command::NewWindow,
    Command::NewFile,
    Command::CloseWindow,
    Command::CloseTab,
    Command::Quit,
    Command::ToggleSidebar,
    Command::GoToFile,
    Command::FindInFiles,
    Command::Replace,
    Command::Find,
    Command::FindPrevious,
    Command::FindNext,
    Command::GoToLine,
    Command::PreviousTab,
    Command::NextTab,
];

pub fn shortcut(command: &Command) -> Option<KeyboardShortcut> {
    let mac = cfg!(target_os = "macos");
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
    let (modifiers, key) = match command {
        Command::NewFile => (cmd, Key::N),
        Command::NewWindow => (cmd_shift, Key::N),
        Command::OpenFile => (cmd, Key::O),
        Command::OpenFolder => (cmd_shift, Key::O),
        Command::OpenRecent(_) => return None,
        Command::Save => (cmd, Key::S),
        Command::SaveAs => (cmd_shift, Key::S),
        Command::CloseTab => (cmd, Key::W),
        Command::CloseWindow => (cmd_shift, Key::W),
        Command::Quit => (cmd, Key::Q),
        Command::Find => (cmd, Key::F),
        Command::Replace if mac => (cmd | Modifiers::ALT, Key::F),
        Command::Replace => (cmd, Key::H),
        Command::FindInFiles => (cmd_shift, Key::F),
        Command::FindNext if mac => (cmd, Key::G),
        Command::FindNext => (Modifiers::NONE, Key::F3),
        Command::FindPrevious if mac => (cmd_shift, Key::G),
        Command::FindPrevious => (Modifiers::SHIFT, Key::F3),
        Command::GoToLine => (Modifiers::CTRL, Key::G),
        Command::GoToFile => (cmd, Key::P),
        Command::ToggleSidebar => (cmd, Key::B),
        Command::NextTab => (Modifiers::CTRL, Key::Tab),
        Command::PreviousTab => (Modifiers::CTRL | Modifiers::SHIFT, Key::Tab),
    };
    Some(KeyboardShortcut::new(modifiers, key))
}

/// What the menus need to know about the window they sit in.
pub struct State<'a> {
    pub has_doc: bool,
    pub has_folder: bool,
    pub sidebar: bool,
    pub recent: &'a [PathBuf],
}

fn item(ui: &mut egui::Ui, picked: &mut Option<Command>, label: &str, command: Command, enabled: bool) {
    let mut button = Button::new(label);
    if let Some(keys) = shortcut(&command) {
        button = button.shortcut_text(ui.ctx().format_shortcut(&keys));
    }
    if ui.add_enabled(enabled, button).clicked() {
        *picked = Some(command);
        ui.close();
    }
}

/// Draw the menu bar across the top of the window. Returns the command
/// chosen this frame, if any.
pub fn bar(ctx: &egui::Context, window: u64, pal: &Palette, state: &State) -> Option<Command> {
    let mut picked = None;
    let frame = Frame::new().fill(pal.strip).inner_margin(Margin::symmetric(6, 2));
    egui::TopBottomPanel::top(Id::new(("menu", window))).frame(frame).show_separator_line(false).show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.set_min_width(230.0);
                let p = &mut picked;
                item(ui, p, "New File", Command::NewFile, true);
                item(ui, p, "New Window", Command::NewWindow, true);
                ui.separator();
                item(ui, p, "Open File…", Command::OpenFile, true);
                item(ui, p, "Open Folder…", Command::OpenFolder, true);
                ui.menu_button("Open Recent", |ui| {
                    if state.recent.is_empty() {
                        ui.add_enabled(false, Button::new("No recent folders"));
                    }
                    for folder in state.recent {
                        let name = folder.file_name().unwrap_or_default().to_string_lossy();
                        let parent = folder.parent().map(|d| d.to_string_lossy()).unwrap_or_default();
                        if ui.add(Button::new(name.as_ref()).shortcut_text(parent.as_ref())).clicked() {
                            *p = Some(Command::OpenRecent(folder.clone()));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                item(ui, p, "Save", Command::Save, state.has_doc);
                item(ui, p, "Save As…", Command::SaveAs, state.has_doc);
                ui.separator();
                item(ui, p, "Close Tab", Command::CloseTab, state.has_doc);
                item(ui, p, "Close Window", Command::CloseWindow, true);
                ui.separator();
                item(ui, p, "Quit", Command::Quit, true);
            });
            ui.menu_button("Find", |ui| {
                ui.set_min_width(230.0);
                let p = &mut picked;
                item(ui, p, "Find…", Command::Find, state.has_doc);
                item(ui, p, "Find Next", Command::FindNext, state.has_doc);
                item(ui, p, "Find Previous", Command::FindPrevious, state.has_doc);
                item(ui, p, "Replace…", Command::Replace, state.has_doc);
                ui.separator();
                item(ui, p, "Find in Files…", Command::FindInFiles, state.has_folder);
                ui.separator();
                item(ui, p, "Go to File…", Command::GoToFile, state.has_folder);
                item(ui, p, "Go to Line…", Command::GoToLine, state.has_doc);
            });
            ui.menu_button("View", |ui| {
                ui.set_min_width(230.0);
                let p = &mut picked;
                let label = if state.sidebar { "Hide Sidebar" } else { "Show Sidebar" };
                item(ui, p, label, Command::ToggleSidebar, true);
                ui.separator();
                item(ui, p, "Next Tab", Command::NextTab, state.has_doc);
                item(ui, p, "Previous Tab", Command::PreviousTab, state.has_doc);
            });
        });
    });
    picked
}
