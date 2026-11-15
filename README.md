# Slate

A small native code editor written in Rust: folder sidebar, tabs, line
numbers, a minimap, and syntax highlighting with switchable colour themes.

    cargo run --release -- path/to/project

The first build takes a few minutes (it compiles the GUI stack); after that
it is seconds. `cargo install --path .` puts a `slate` binary on your PATH.

    slate [--theme NAME] [FOLDER | FILE]...
    slate --export-icons

### As a macOS app

    ./bundle-macos.sh --install

builds `target/Slate.app` with an icon and copies it to `/Applications`
(leave off `--install` to only build it). It needs nothing beyond Rust and
the tools macOS ships with. Run it again after changing the code.

The app registers as an editor for text files and folders, so Finder's
*Open With → Slate* works, as does dropping a file or folder on the Dock
icon. A file opens in the window whose project contains it. A file from
anywhere else opens in a separate window with no sidebar, shared with any
other such files, so it never appears under an unrelated project. A folder
gets its own window. Slate makes itself the default app for `.rs` files only (macOS has no
built-in type for them, so Slate defines one); for other types, choose it in
Finder's *Get Info* if you want it; such files then show Slate's document icon (`assets/DocumentIcon.png`)
in Finder. The receiving end is `src/macos_open.m` and `src/open_events.rs`.

With no arguments it reopens the folders that were open last time. Several
folders on the command line open one window each.

## Windows and projects

One window is one project. *File → Open Folder…* (or a folder dropped on the
window) opens in the current window if it is empty, and in a new window
otherwise, so two or three projects sit side by side, each with its own
sidebar, tabs, find bar and search results. *File → Open Recent* lists the
last twelve folders; picking one that is already open brings its window
forward. *New Window* opens an empty one.

Closing a window (its close button, or Cmd+Shift+W) asks about that window's
unsaved files. Quitting asks once about unsaved files in all windows. The
theme is shared: changing it in one window changes them all.

## Menus

A menu bar runs along the top of every window: **File** (new, open, open
recent, save, close, quit), **Find** (find, replace, find in files, go to
file, go to line) and **View** (sidebar, tabs). Each item shows its shortcut.
Menus and shortcuts run the same commands, defined in one table in
`src/menu.rs`. On macOS the bar is inside the window rather than in the
system menu bar at the top of the screen.

## Highlighting and themes

Highlighting uses [syntect], which reads the same `.sublime-syntax` grammars
and `.tmTheme` colour schemes as Sublime Text. Rust, TOML, Markdown and about
200 other languages are bundled. The language is picked from the file name
and can be overridden from the menu at the right of the status bar.

Themes are chosen from the status bar too; the choice is remembered. About
thirty are built in (Monokai Extended is the default; also Dracula, Nord,
Gruvbox, One Half, Solarized, GitHub, ...). To add your own, drop any
`.tmTheme` file into the themes folder and restart:

| OS      | Folder                                          |
|---------|-------------------------------------------------|
| macOS   | `~/Library/Application Support/slate/themes`    |
| Linux   | `~/.local/share/slate/themes`                   |
| Windows | `%APPDATA%\slate\themes`                        |

The sidebar, tab strip, gutter and selection colours are derived from the
theme, so the whole window follows it.

## Sidebar icons

Files and folders get an icon from a built-in sprite sheet
(`assets/icons.png`). To use your own art, put an `icons.png` in the
settings folder (the parent of the themes folder above, e.g.
`~/Library/Application Support/slate/icons.png`). The quickest start is

    slate --export-icons

which writes the built-in sheet there for you to paint over. Slate re-reads
the file whenever its window regains focus, so you can edit and flip back.

The sheet is a grid 8 cells wide. Cells are square and their size is the
image width divided by 8, so 128 px wide means 16 px icons, 256 means 32 px
(the built-in size, sharp on Retina), 512 means 64 px. Icons are always
drawn at 16 points. Each cell belongs to one icon:

|       | 0      | 1           | 2     | 3    | 4      | 5          | 6      | 7       |
|-------|--------|-------------|-------|------|--------|------------|--------|---------|
| row 0 | folder | folder open | file  | Rust | TOML   | Markdown   | JSON   | YAML    |
| row 1 | text   | image       | shell | lock | git    | config     | shader | C       |
| row 2 | C++    | Python      | JS    | TS   | HTML   | CSS        | audio  | archive |

"file" is the fallback for anything unrecognised. A cell you leave fully
transparent keeps the built-in icon, and a sheet with fewer rows is fine, so
you can replace just the few you care about. Which file names map to which
icon is the `match` in `src/icons.rs`; adding a new icon is a new `Icon`
variant (appended, so existing sheets stay valid), a line in that `match`,
and a cell in the sheet. `assets/make_icons.py` redraws the built-in sheet.

## Find, replace, find in files

**Find** (Cmd+F) outlines every match and steps through them with Enter.
`Aa` matches case, `W` matches whole words only.

**Replace** (Cmd+Option+F on macOS, Ctrl+H elsewhere) adds a second field;
Tab moves between the two. Enter or *Replace* swaps the selected match and
moves to the next. *Replace All* changes every match in the file as one
undo step.

**Find in files** (Cmd+Shift+F) searches the open folder in the background
and lists matching lines grouped by file as they are found. Click a line to
open the file there; click a file name to fold its results. Tabs with
unsaved changes are searched as they are in the editor. `.git`, `target*`
and `node_modules` are skipped, as are binary files and files over 4 MB.

## Keys

`Cmd` below is `Ctrl` on Linux and Windows.

| Key                          | Action                                  |
|------------------------------|-----------------------------------------|
| Cmd+P                        | Go to file (fuzzy)                      |
| Cmd+O / Cmd+Shift+O          | Open file / open folder                 |
| Cmd+S / Cmd+Shift+S          | Save / save as                          |
| Cmd+N / Cmd+W                | New tab / close tab                     |
| Cmd+Shift+N / Cmd+Shift+W    | New window / close window               |
| Cmd+Q                        | Quit                                    |
| Ctrl+Tab / Ctrl+Shift+Tab    | Next / previous tab                     |
| Cmd+B                        | Show or hide the sidebar                |
| Cmd+F                        | Find; Enter / Shift+Enter to step       |
| Cmd+Option+F (Ctrl+H)        | Find and replace                        |
| Cmd+Shift+F                  | Find in files                           |
| F3 / Shift+F3 (Cmd+G on Mac) | Next / previous match                   |
| Ctrl+G                       | Go to line (`120` or `120:9`)           |
| Cmd+/                        | Toggle line comment                     |
| Tab / Shift+Tab, Cmd+] / [   | Indent / outdent selected lines         |
| Cmd+Shift+D / Cmd+Shift+K    | Duplicate / delete line                 |
| Cmd+L                        | Select line                             |
| Cmd+Enter                    | New line below                          |
| Cmd+Z / Cmd+Shift+Z          | Undo / redo                             |
| Cmd + / Cmd - / Cmd 0        | Zoom                                    |

Word and line movement follow the platform: Option+arrows and Cmd+arrows on
macOS, Ctrl+arrows and Home/End elsewhere. Double-click selects a word,
triple-click or a click in the gutter selects a line. Click or drag in the
minimap to scroll.

## Source layout

| File               | What it holds                                           |
|--------------------|---------------------------------------------------------|
| `src/buffer.rs`    | Text, cursor, selection, editing commands, undo         |
| `src/highlight.rs` | Incremental highlighter (only re-reads what an edit can affect) |
| `src/theme.rs`     | Theme loading and the chrome palette derived from a theme |
| `src/editor.rs`    | The painted editor: gutter, text, caret, minimap, input |
| `src/tree.rs`      | Folder sidebar, file index, fuzzy matching              |
| `src/search.rs`    | Find in files: background search and its results        |
| `src/icons.rs`     | Icon sprite sheets: built-in, custom, name-to-icon map  |
| `src/workspace.rs` | One window: tabs, status bar, find, dialogs             |
| `src/menu.rs`      | Commands, their shortcuts, the menu bar                 |
| `src/main.rs`      | The set of windows and what they share; startup         |

`cargo test` covers the buffer, the highlighter, searching and the icon mapping.

## Not there yet

Multiple cursors, regex search, replace across files, bracket auto-closing,
soft wrap, split panes, a native macOS menu bar, and reloading files changed on disk by another
program.

[syntect]: https://github.com/trishume/syntect
