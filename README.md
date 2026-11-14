# Slate

A small native code editor written in Rust: folder sidebar, tabs, line
numbers, a minimap, and syntax highlighting with switchable colour themes.

    cargo run --release -- path/to/project

The first build takes a few minutes (it compiles the GUI stack); after that
it is seconds. `cargo install --path .` puts a `slate` binary on your PATH.

    slate [--theme NAME] [FOLDER | FILE]...

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
| `src/main.rs`      | Window, tabs, status bar, find, dialogs, shortcuts      |

`cargo test` covers the buffer and the highlighter.

## Not there yet

Multiple cursors, find-and-replace, find in files, bracket auto-closing,
soft wrap, split panes, and reloading files changed on disk by another
program.

[syntect]: https://github.com/trishume/syntect
