# Slate

A small native code editor written in Rust: folder sidebar, tabs, line
numbers, a minimap, and syntax highlighting with switchable colour themes.

    cargo run --release -- path/to/project

The first build takes a few minutes (it compiles the GUI stack); after that
it is seconds. `cargo install --path .` puts a `slate` binary on your PATH.

    slate [--theme NAME] [FOLDER | FILE]...
    slate --export-icons

With no arguments it reopens the last folder.

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

## Keys

`Cmd` below is `Ctrl` on Linux and Windows.

| Key                          | Action                                  |
|------------------------------|-----------------------------------------|
| Cmd+P                        | Go to file (fuzzy)                      |
| Cmd+O / Cmd+Shift+O          | Open file / open folder                 |
| Cmd+S / Cmd+Shift+S          | Save / save as                          |
| Cmd+N / Cmd+W                | New tab / close tab                     |
| Ctrl+Tab / Ctrl+Shift+Tab    | Next / previous tab                     |
| Cmd+B                        | Show or hide the sidebar                |
| Cmd+F                        | Find; Enter / Shift+Enter to step       |
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
| `src/icons.rs`     | Icon sprite sheets: built-in, custom, name-to-icon map  |
| `src/main.rs`      | Window, tabs, status bar, find, dialogs, shortcuts      |

`cargo test` covers the buffer, the highlighter and the icon mapping.

## Not there yet

Multiple cursors, find-and-replace, find in files, bracket auto-closing,
soft wrap, split panes, and reloading files changed on disk by another
program.

[syntect]: https://github.com/trishume/syntect
