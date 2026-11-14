//! Incremental syntax highlighting on top of syntect.
//!
//! Every line remembers the parser state it ended in. An edit marks its
//! lines as unknown; re-highlighting then runs forward from there and stops
//! as soon as a line ends in the same state it did before, because nothing
//! below it can have changed. Work is done in small time slices so a big
//! file never blocks a frame.

use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::Color32;
use syntect::highlighting::{
    FontStyle, HighlightState, Highlighter as Styles, RangedHighlightIterator, Theme,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use crate::buffer::Change;

/// Lines longer than this are shown plain; they are almost always
/// generated data and would stall the regex engine.
const MAX_LINE: usize = 4000;

#[derive(Clone, Copy)]
pub struct Span {
    /// Length in bytes.
    pub len: u32,
    pub color: Color32,
    pub italic: bool,
}

#[derive(Default)]
struct Line {
    spans: Vec<Span>,
    /// State after this line, or `None` if it has to be worked out again.
    state: Option<(ParseState, HighlightState)>,
}

pub struct Highlighter {
    pub syntax: String,
    lines: Vec<Line>,
    /// Everything before this line is up to date.
    dirty_from: usize,
}

impl Highlighter {
    pub fn new(syntax: &str, line_count: usize) -> Self {
        Self {
            syntax: syntax.to_string(),
            lines: (0..line_count).map(|_| Line::default()).collect(),
            dirty_from: 0,
        }
    }

    pub fn apply(&mut self, c: &Change) {
        let end = (c.line + c.old).min(self.lines.len());
        self.lines
            .splice(c.line..end, (0..c.new).map(|_| Line::default()));
        self.dirty_from = self.dirty_from.min(c.line);
    }

    /// Theme or syntax changed: redo everything, but keep showing the old
    /// colours until each line's turn comes.
    pub fn invalidate(&mut self) {
        for l in &mut self.lines {
            l.state = None;
        }
        self.dirty_from = 0;
    }

    pub fn set_syntax(&mut self, name: &str) {
        self.syntax = name.to_string();
        self.invalidate();
    }

    pub fn spans(&self, line: usize) -> &[Span] {
        self.lines.get(line).map_or(&[], |l| &l.spans)
    }

    /// Highlight for at most `budget`. Returns true if work remains.
    pub fn run(&mut self, text: &[String], ss: &SyntaxSet, theme: &Theme, budget: Duration) -> bool {
        if self.lines.len() != text.len() {
            // Should not happen; recover rather than index out of bounds.
            self.lines = (0..text.len()).map(|_| Line::default()).collect();
            self.dirty_from = 0;
        }
        if self.dirty_from >= self.lines.len() {
            return false;
        }
        let syntax = ss
            .find_syntax_by_name(&self.syntax)
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        let styles = Styles::new(theme);
        let started = Instant::now();
        let mut buf = String::new();

        while self.dirty_from < self.lines.len() {
            let i = self.dirty_from;
            let (mut parse, mut hl) = match i.checked_sub(1).and_then(|p| self.lines[p].state.clone())
            {
                Some(s) => s,
                None => (
                    ParseState::new(syntax),
                    HighlightState::new(&styles, ScopeStack::new()),
                ),
            };

            let line = &text[i];
            let mut spans: Vec<Span> = Vec::new();
            if line.len() <= MAX_LINE {
                buf.clear();
                buf.push_str(line);
                buf.push('\n');
                let ops = parse.parse_line(&buf, ss).unwrap_or_default();
                for (style, _, range) in RangedHighlightIterator::new(&mut hl, &ops, &buf, &styles) {
                    let len = range.end.min(line.len()).saturating_sub(range.start) as u32;
                    if len == 0 {
                        continue;
                    }
                    let f = style.foreground;
                    let color = Color32::from_rgb(f.r, f.g, f.b);
                    let italic = style.font_style.contains(FontStyle::ITALIC);
                    match spans.last_mut() {
                        Some(s) if s.color == color && s.italic == italic => s.len += len,
                        _ => spans.push(Span { len, color, italic }),
                    }
                }
            }

            let state = Some((parse, hl));
            let settled = self.lines[i].state == state;
            self.lines[i] = Line { spans, state };
            self.dirty_from = if settled {
                // Same end state as before: skip ahead to the next edit.
                (i + 1..self.lines.len())
                    .find(|j| self.lines[*j].state.is_none())
                    .unwrap_or(self.lines.len())
            } else {
                i + 1
            };

            if started.elapsed() >= budget {
                break;
            }
        }
        self.dirty_from < self.lines.len()
    }
}

/// Pick a syntax from the file name, then the extension, then the first line.
pub fn syntax_for<'a>(ss: &'a SyntaxSet, path: Option<&Path>, first_line: &str) -> &'a SyntaxReference {
    let by_path = path.and_then(|p| {
        let name = p.file_name()?.to_str()?;
        ss.find_syntax_by_extension(name).or_else(|| {
            let ext = p.extension()?.to_str()?;
            ss.find_syntax_by_extension(ext)
                .or_else(|| ss.find_syntax_by_extension(&ext.to_lowercase()))
        })
    });
    by_path
        .or_else(|| ss.find_syntax_by_first_line(first_line))
        .unwrap_or_else(|| ss.find_syntax_plain_text())
}

/// The line-comment marker for a syntax, if it has one.
pub fn comment_prefix(syntax: &str) -> Option<&'static str> {
    let s = syntax.to_lowercase();
    let has = |names: &[&str]| names.iter().any(|n| s.contains(n));
    if has(&["markdown", "plain text", "html", "xml", "css", "json"]) && !has(&["jsonc", "scss"]) {
        None
    } else if has(&[
        "toml", "yaml", "python", "shell", "bash", "make", "ruby", "docker", "perl", "nix",
        "cmake", "fish", "elixir", "julia", "powershell", "git", "properties", "nginx",
    ]) || s == "r"
    {
        Some("#")
    } else if has(&["lua", "sql", "haskell", "ada", "elm"]) {
        Some("--")
    } else if has(&["lisp", "clojure", "scheme", "ini", "asm"]) {
        Some(";")
    } else if has(&["tex", "erlang", "matlab"]) {
        Some("%")
    } else {
        Some("//")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Buffer, EditKind, Pos};

    fn setup() -> (SyntaxSet, Theme) {
        let themes = crate::theme::Themes::load(None);
        (two_face::syntax::extra_newlines(), themes.list[0].1.clone())
    }

    fn colours(h: &Highlighter, n: usize) -> Vec<Vec<(u32, Color32)>> {
        (0..n)
            .map(|i| h.spans(i).iter().map(|s| (s.len, s.color)).collect())
            .collect()
    }

    const SRC: &str = "use std::fmt;\n\n/// Doc\npub fn add(a: u32, b: u32) -> u32 {\n    let s = \"text\";\n    a + b // sum\n}\n\nconst X: f32 = 1.0;\n";

    #[test]
    fn rust_is_coloured_and_spans_cover_lines() {
        let (ss, theme) = setup();
        let b = Buffer::from_text(SRC);
        let name = syntax_for(&ss, Some(Path::new("a.rs")), "").name.clone();
        assert_eq!(name, "Rust");
        let mut h = Highlighter::new(&name, b.lines.len());
        while h.run(&b.lines, &ss, &theme, Duration::from_millis(50)) {}
        for (i, l) in b.lines.iter().enumerate() {
            let total: u32 = h.spans(i).iter().map(|s| s.len).sum();
            assert_eq!(total as usize, l.len(), "line {i}");
        }
        // `pub fn add(...)` has more than one colour in it.
        assert!(h.spans(3).len() > 3);
    }

    #[test]
    fn incremental_matches_full_rehighlight() {
        let (ss, theme) = setup();
        let mut b = Buffer::from_text(SRC);
        let mut h = Highlighter::new("Rust", b.lines.len());
        while h.run(&b.lines, &ss, &theme, Duration::from_millis(50)) {}
        b.changes.clear();

        // Open a block comment on line 2: everything below changes...
        b.move_to(Pos::new(1, 0), false);
        b.insert("/*", EditKind::Other);
        // ...then a multi-line paste further down, before catching up.
        b.move_to(Pos::new(5, 0), false);
        b.insert("    let y = 2;\n    let z = 3;\n", EditKind::Other);
        // ...and close the comment again.
        b.move_to(Pos::new(1, 2), false);
        b.insert(" x */", EditKind::Other);
        for c in b.changes.drain(..) {
            h.apply(&c);
        }
        while h.run(&b.lines, &ss, &theme, Duration::from_micros(1)) {}

        let mut fresh = Highlighter::new("Rust", b.lines.len());
        while fresh.run(&b.lines, &ss, &theme, Duration::from_millis(50)) {}
        assert_eq!(colours(&h, b.lines.len()), colours(&fresh, b.lines.len()));
    }

    #[test]
    fn finds_toml_and_markdown() {
        let (ss, _) = setup();
        assert_eq!(syntax_for(&ss, Some(Path::new("bake.toml")), "").name, "TOML");
        assert_eq!(syntax_for(&ss, Some(Path::new("ZONES.md")), "").name, "Markdown");
        assert_eq!(syntax_for(&ss, Some(Path::new("x.sh")), "").name, "Bourne Again Shell (bash)");
        assert_eq!(comment_prefix("TOML"), Some("#"));
        assert_eq!(comment_prefix("Rust"), Some("//"));
        assert_eq!(comment_prefix("Markdown"), None);
    }
}
