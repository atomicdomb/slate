//! The text itself: lines, a cursor with a selection anchor, and undo.
//!
//! Lines are stored without their terminators. Columns are counted in
//! chars, not bytes, so a `Pos` can be handed straight to the text layout.

use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    /// Single typed characters; consecutive ones undo together.
    Typing,
    Other,
}

/// "Lines `line .. line + old` became `new` lines." The highlighter
/// consumes these to know what to re-read.
#[derive(Clone, Copy, Debug)]
pub struct Change {
    pub line: usize,
    pub old: usize,
    pub new: usize,
}

struct Edit {
    start: Pos,
    removed: String,
    inserted: String,
}

struct Group {
    edits: Vec<Edit>,
    kind: EditKind,
    before: (Pos, Pos),
    after: (Pos, Pos),
    time: Instant,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Indent {
    Tabs,
    Spaces(usize),
}

impl Indent {
    pub fn unit(&self) -> String {
        match self {
            Indent::Tabs => "\t".to_string(),
            Indent::Spaces(n) => " ".repeat(*n),
        }
    }
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub cursor: Pos,
    pub anchor: Pos,
    /// The column up/down movement tries to return to.
    goal_col: Option<usize>,
    undo: Vec<Group>,
    redo: Vec<Group>,
    current: Option<Group>,
    pub changes: Vec<Change>,
    pub dirty: bool,
    pub crlf: bool,
    pub indent: Indent,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

fn detect_indent(lines: &[String]) -> Indent {
    let mut smallest = usize::MAX;
    for l in lines.iter().take(400) {
        if l.starts_with('\t') {
            return Indent::Tabs;
        }
        let n = l.len() - l.trim_start_matches(' ').len();
        if n >= 2 && n < l.len() {
            smallest = smallest.min(n);
        }
    }
    match smallest {
        2 | 3 => Indent::Spaces(2),
        8 => Indent::Spaces(8),
        _ => Indent::Spaces(4),
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let crlf = text.contains("\r\n");
        let lines: Vec<String> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        let indent = detect_indent(&lines);
        Self {
            lines,
            cursor: Pos::default(),
            anchor: Pos::default(),
            goal_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            current: None,
            changes: Vec::new(),
            dirty: false,
            crlf,
            indent,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join(if self.crlf { "\r\n" } else { "\n" })
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.lines[line].chars().count()
    }

    fn byte(&self, p: Pos) -> usize {
        let s = &self.lines[p.line];
        s.char_indices().nth(p.col).map_or(s.len(), |(i, _)| i)
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(self.line_len(line)))
    }

    fn last(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos::new(line, self.line_len(line))
    }

    pub fn selection(&self) -> Option<(Pos, Pos)> {
        if self.cursor == self.anchor {
            None
        } else {
            Some((self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
        }
    }

    pub fn text_in(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            return self.lines[a.line][self.byte(a)..self.byte(b)].to_string();
        }
        let mut out = self.lines[a.line][self.byte(a)..].to_string();
        for l in &self.lines[a.line + 1..b.line] {
            out.push('\n');
            out.push_str(l);
        }
        out.push('\n');
        out.push_str(&self.lines[b.line][..self.byte(b)]);
        out
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.text_in(a, b))
    }

    /// Where a cursor at `start` lands after `text` is inserted there.
    fn end_of(start: Pos, text: &str) -> Pos {
        match text.rfind('\n') {
            None => Pos::new(start.line, start.col + text.chars().count()),
            Some(i) => Pos::new(
                start.line + text.matches('\n').count(),
                text[i + 1..].chars().count(),
            ),
        }
    }

    fn raw_replace(&mut self, a: Pos, b: Pos, text: &str) -> (String, Pos) {
        let removed = self.text_in(a, b);
        let head = self.lines[a.line][..self.byte(a)].to_string();
        let tail = self.lines[b.line][self.byte(b)..].to_string();
        let mut new: Vec<String> = text.split('\n').map(String::from).collect();
        let n = new.len();
        new[0].insert_str(0, &head);
        new[n - 1].push_str(&tail);
        self.changes.push(Change {
            line: a.line,
            old: b.line - a.line + 1,
            new: n,
        });
        self.lines.splice(a.line..=b.line, new);
        self.dirty = true;
        (removed, Self::end_of(a, text))
    }

    /// Replace `a..b` with `text`, recording it for undo. Must be called
    /// inside [`Buffer::edit`]. Returns the end of the inserted text.
    fn replace(&mut self, a: Pos, b: Pos, text: &str) -> Pos {
        let (removed, end) = self.raw_replace(a, b, text);
        if let Some(g) = &mut self.current {
            g.edits.push(Edit {
                start: a,
                removed,
                inserted: text.to_string(),
            });
        }
        end
    }

    /// Run `f` as one undo step.
    fn edit(&mut self, kind: EditKind, f: impl FnOnce(&mut Self)) {
        let here = (self.cursor, self.anchor);
        let now = Instant::now();
        let continues = kind == EditKind::Typing
            && self.undo.last().is_some_and(|g| {
                g.kind == EditKind::Typing
                    && g.after == here
                    && now.duration_since(g.time).as_millis() < 1000
            });
        let mut group = if continues {
            self.undo.pop().unwrap()
        } else {
            Group {
                edits: Vec::new(),
                kind,
                before: here,
                after: here,
                time: now,
            }
        };
        group.time = now;
        self.current = Some(group);
        f(self);
        let mut group = self.current.take().unwrap();
        group.after = (self.cursor, self.anchor);
        if !group.edits.is_empty() {
            self.undo.push(group);
            self.redo.clear();
        }
        self.goal_col = None;
    }

    pub fn undo(&mut self) {
        let Some(g) = self.undo.pop() else { return };
        for e in g.edits.iter().rev() {
            let end = Self::end_of(e.start, &e.inserted);
            self.raw_replace(e.start, end, &e.removed);
        }
        (self.cursor, self.anchor) = g.before;
        self.goal_col = None;
        self.redo.push(g);
    }

    pub fn redo(&mut self) {
        let Some(g) = self.redo.pop() else { return };
        for e in &g.edits {
            let end = Self::end_of(e.start, &e.removed);
            self.raw_replace(e.start, end, &e.inserted);
        }
        (self.cursor, self.anchor) = g.after;
        self.goal_col = None;
        self.undo.push(g);
    }

    // ------------------------------------------------------------ editing

    fn insert_in_group(&mut self, text: &str) {
        let (a, b) = self.selection().unwrap_or((self.cursor, self.cursor));
        let end = self.replace(a, b, text);
        self.cursor = end;
        self.anchor = end;
    }

    /// Insert at the cursor, replacing any selection.
    pub fn insert(&mut self, text: &str, kind: EditKind) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.edit(kind, |b| b.insert_in_group(&text));
    }

    fn delete_range(&mut self, a: Pos, b: Pos) {
        self.edit(EditKind::Other, |buf| {
            let end = buf.replace(a, b, "");
            buf.cursor = end;
            buf.anchor = end;
        });
    }

    pub fn backspace(&mut self) {
        if let Some((a, b)) = self.selection() {
            return self.delete_range(a, b);
        }
        let c = self.cursor;
        // Inside space indentation, step back a whole indent level.
        if let Indent::Spaces(w) = self.indent {
            let before = &self.lines[c.line][..self.byte(c)];
            if c.col > 0 && before.chars().all(|ch| ch == ' ') {
                let n = (c.col - 1) % w + 1;
                return self.delete_range(Pos::new(c.line, c.col - n), c);
            }
        }
        let from = self.left_of(c);
        self.delete_range(from, c);
    }

    pub fn delete_forward(&mut self) {
        match self.selection() {
            Some((a, b)) => self.delete_range(a, b),
            None => {
                let to = self.right_of(self.cursor);
                self.delete_range(self.cursor, to);
            }
        }
    }

    pub fn delete_word_back(&mut self) {
        match self.selection() {
            Some((a, b)) => self.delete_range(a, b),
            None => {
                let from = self.word_left(self.cursor);
                self.delete_range(from, self.cursor);
            }
        }
    }

    pub fn delete_word_forward(&mut self) {
        match self.selection() {
            Some((a, b)) => self.delete_range(a, b),
            None => {
                let to = self.word_right(self.cursor);
                self.delete_range(self.cursor, to);
            }
        }
    }

    pub fn delete_to_line_start(&mut self) {
        let c = self.cursor;
        self.delete_range(Pos::new(c.line, 0), c);
    }

    fn leading_ws(&self, line: usize) -> String {
        self.lines[line]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// Enter: carry the indentation over, and open a block after `{ ( [`.
    pub fn newline(&mut self) {
        let unit = self.indent.unit();
        self.edit(EditKind::Other, |b| {
            let (start, end) = b.selection().unwrap_or((b.cursor, b.cursor));
            let mut ws = b.leading_ws(start.line);
            ws.truncate(ws.len().min(b.byte(start)));
            let before = b.lines[start.line][..b.byte(start)].trim_end();
            let after = b.lines[end.line][b.byte(end)..].trim_start();
            let opener = before.chars().last();
            let closes = matches!(
                (opener, after.chars().next()),
                (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
            );
            if matches!(opener, Some('{' | '(' | '[')) {
                let inner = format!("\n{ws}{unit}");
                let at = b.replace(start, end, &inner);
                if closes {
                    b.replace(at, at, &format!("\n{ws}"));
                }
                b.cursor = at;
            } else {
                b.cursor = b.replace(start, end, &format!("\n{ws}"));
            }
            b.anchor = b.cursor;
        });
    }

    /// The lines a line-wise command applies to. A selection that ends at
    /// column 0 does not include that last line.
    fn selected_lines(&self) -> (usize, usize) {
        match self.selection() {
            None => (self.cursor.line, self.cursor.line),
            Some((a, b)) if b.col == 0 && b.line > a.line => (a.line, b.line - 1),
            Some((a, b)) => (a.line, b.line),
        }
    }

    pub fn tab(&mut self) {
        if self.selection().is_some_and(|(a, b)| a.line != b.line) {
            return self.indent_lines();
        }
        let text = match self.indent {
            Indent::Tabs => "\t".to_string(),
            Indent::Spaces(w) => {
                let col = self.selection().map_or(self.cursor.col, |(a, _)| a.col);
                " ".repeat(w - col % w)
            }
        };
        self.insert(&text, EditKind::Other);
    }

    pub fn indent_lines(&mut self) {
        let unit = self.indent.unit();
        let n = unit.chars().count();
        let (first, last) = self.selected_lines();
        self.edit(EditKind::Other, |b| {
            for line in first..=last {
                if b.lines[line].is_empty() {
                    continue;
                }
                b.replace(Pos::new(line, 0), Pos::new(line, 0), &unit);
                for p in [&mut b.cursor, &mut b.anchor] {
                    if p.line == line && (p.col > 0 || first == last) {
                        p.col += n;
                    }
                }
            }
        });
    }

    pub fn outdent_lines(&mut self) {
        let width = match self.indent {
            Indent::Tabs => 1,
            Indent::Spaces(w) => w,
        };
        let (first, last) = self.selected_lines();
        self.edit(EditKind::Other, |b| {
            for line in first..=last {
                let s = &b.lines[line];
                let n = if s.starts_with('\t') {
                    1
                } else {
                    s.chars().take(width).take_while(|c| *c == ' ').count()
                };
                if n == 0 {
                    continue;
                }
                b.replace(Pos::new(line, 0), Pos::new(line, n), "");
                for p in [&mut b.cursor, &mut b.anchor] {
                    if p.line == line {
                        p.col = p.col.saturating_sub(n);
                    }
                }
            }
        });
    }

    /// Comment the selected lines out, or back in if they all already are.
    pub fn toggle_comment(&mut self, prefix: &str) {
        let (first, last) = self.selected_lines();
        let plen = prefix.chars().count();
        let live: Vec<usize> = (first..=last)
            .filter(|l| !self.lines[*l].trim().is_empty())
            .collect();
        if live.is_empty() {
            return;
        }
        let indent_of = |b: &Self, l: usize| b.leading_ws(l).chars().count();
        let all_commented = live
            .iter()
            .all(|l| self.lines[*l].trim_start().starts_with(prefix));
        let at = live.iter().map(|l| indent_of(self, *l)).min().unwrap();
        self.edit(EditKind::Other, |b| {
            for &line in &live {
                if all_commented {
                    let col = indent_of(b, line);
                    let rest: String = b.lines[line].chars().skip(col + plen).collect();
                    let n = plen + usize::from(rest.starts_with(' '));
                    b.replace(Pos::new(line, col), Pos::new(line, col + n), "");
                    for p in [&mut b.cursor, &mut b.anchor] {
                        if p.line == line && p.col > col {
                            p.col = p.col.saturating_sub(n).max(col);
                        }
                    }
                } else {
                    let text = format!("{prefix} ");
                    b.replace(Pos::new(line, at), Pos::new(line, at), &text);
                    for p in [&mut b.cursor, &mut b.anchor] {
                        if p.line == line && p.col >= at {
                            p.col += plen + 1;
                        }
                    }
                }
            }
        });
    }

    /// Replace several ranges with the same text as one undo step. The
    /// ranges must be in document order and must not overlap.
    pub fn replace_ranges(&mut self, ranges: &[(Pos, Pos)], text: &str) {
        self.edit(EditKind::Other, |b| {
            // Back to front, so earlier ranges keep their positions.
            for (from, to) in ranges.iter().rev() {
                b.replace(*from, *to, text);
            }
            b.cursor = b.clamp(b.cursor);
            b.anchor = b.cursor;
        });
    }

    pub fn duplicate_line(&mut self) {
        let (first, last) = self.selected_lines();
        let block = self.lines[first..=last].join("\n");
        let count = last - first + 1;
        self.edit(EditKind::Other, |b| {
            let end = Pos::new(last, b.line_len(last));
            b.replace(end, end, &format!("\n{block}"));
            b.cursor.line += count;
            b.anchor.line += count;
        });
    }

    pub fn delete_line(&mut self) {
        let (first, last) = self.selected_lines();
        self.edit(EditKind::Other, |b| {
            let (a, e) = if last + 1 < b.lines.len() {
                (Pos::new(first, 0), Pos::new(last + 1, 0))
            } else if first > 0 {
                (Pos::new(first - 1, b.line_len(first - 1)), b.last())
            } else {
                (Pos::new(0, 0), b.last())
            };
            b.replace(a, e, "");
            b.cursor = b.clamp(Pos::new(first, 0));
            b.anchor = b.cursor;
        });
    }

    /// Cut with nothing selected takes the whole line, as Sublime does.
    pub fn cut(&mut self) -> String {
        match self.selection() {
            Some((a, b)) => {
                let text = self.text_in(a, b);
                self.delete_range(a, b);
                text
            }
            None => {
                let text = format!("{}\n", self.lines[self.cursor.line]);
                self.delete_line();
                text
            }
        }
    }

    pub fn copy(&self) -> String {
        self.selected_text()
            .unwrap_or_else(|| format!("{}\n", self.lines[self.cursor.line]))
    }

    // ----------------------------------------------------------- movement

    pub fn move_to(&mut self, p: Pos, extend: bool) {
        self.cursor = self.clamp(p);
        if !extend {
            self.anchor = self.cursor;
        }
        self.goal_col = None;
    }

    fn left_of(&self, p: Pos) -> Pos {
        if p.col > 0 {
            Pos::new(p.line, p.col - 1)
        } else if p.line > 0 {
            Pos::new(p.line - 1, self.line_len(p.line - 1))
        } else {
            p
        }
    }

    fn right_of(&self, p: Pos) -> Pos {
        if p.col < self.line_len(p.line) {
            Pos::new(p.line, p.col + 1)
        } else if p.line + 1 < self.lines.len() {
            Pos::new(p.line + 1, 0)
        } else {
            p
        }
    }

    fn word_left(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return self.left_of(p);
        }
        let chars: Vec<char> = self.lines[p.line].chars().collect();
        let mut i = p.col;
        while i > 0 && class(chars[i - 1]) == Class::Space {
            i -= 1;
        }
        if i > 0 {
            let c = class(chars[i - 1]);
            while i > 0 && class(chars[i - 1]) == c {
                i -= 1;
            }
        }
        Pos::new(p.line, i)
    }

    fn word_right(&self, p: Pos) -> Pos {
        let chars: Vec<char> = self.lines[p.line].chars().collect();
        if p.col >= chars.len() {
            return self.right_of(p);
        }
        let mut i = p.col;
        while i < chars.len() && class(chars[i]) == Class::Space {
            i += 1;
        }
        if i < chars.len() {
            let c = class(chars[i]);
            while i < chars.len() && class(chars[i]) == c {
                i += 1;
            }
        }
        Pos::new(p.line, i)
    }

    pub fn move_left(&mut self, extend: bool) {
        let to = match self.selection() {
            Some((a, _)) if !extend => a,
            _ => self.left_of(self.cursor),
        };
        self.move_to(to, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let to = match self.selection() {
            Some((_, b)) if !extend => b,
            _ => self.right_of(self.cursor),
        };
        self.move_to(to, extend);
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.move_to(self.word_left(self.cursor), extend);
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.move_to(self.word_right(self.cursor), extend);
    }

    pub fn move_lines(&mut self, delta: isize, extend: bool) {
        let goal = self.goal_col.unwrap_or(self.cursor.col);
        let line = self.cursor.line as isize + delta;
        let to = if line < 0 {
            Pos::new(0, 0)
        } else if line as usize >= self.lines.len() {
            self.last()
        } else {
            Pos::new(line as usize, goal)
        };
        self.move_to(to, extend);
        self.goal_col = Some(goal);
    }

    /// Home: first to the start of the code, then to column 0.
    pub fn move_home(&mut self, extend: bool) {
        let ws = self.leading_ws(self.cursor.line).chars().count();
        let col = if self.cursor.col == ws { 0 } else { ws };
        self.move_to(Pos::new(self.cursor.line, col), extend);
    }

    pub fn move_end(&mut self, extend: bool) {
        self.move_to(Pos::new(self.cursor.line, usize::MAX), extend);
    }

    pub fn move_doc_start(&mut self, extend: bool) {
        self.move_to(Pos::default(), extend);
    }

    pub fn move_doc_end(&mut self, extend: bool) {
        self.move_to(self.last(), extend);
    }

    pub fn select_all(&mut self) {
        self.anchor = Pos::default();
        self.cursor = self.last();
    }

    pub fn word_at(&self, p: Pos) -> (Pos, Pos) {
        let chars: Vec<char> = self.lines[p.line].chars().collect();
        if chars.is_empty() {
            return (p, p);
        }
        let i = p.col.min(chars.len() - 1);
        let c = class(chars[i]);
        let mut a = i;
        while a > 0 && class(chars[a - 1]) == c {
            a -= 1;
        }
        let mut b = i;
        while b < chars.len() && class(chars[b]) == c {
            b += 1;
        }
        (Pos::new(p.line, a), Pos::new(p.line, b))
    }

    pub fn select_line(&mut self, line: usize) {
        self.anchor = Pos::new(line, 0);
        self.cursor = if line + 1 < self.lines.len() {
            Pos::new(line + 1, 0)
        } else {
            Pos::new(line, self.line_len(line))
        };
        self.goal_col = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Buffer {
        Buffer::from_text(s)
    }

    #[test]
    fn insert_and_undo_round_trip() {
        let mut b = buf("fn main() {\n}\n");
        b.move_to(Pos::new(0, 11), false);
        b.insert("\n    let x = 1;", EditKind::Other);
        assert_eq!(b.text(), "fn main() {\n    let x = 1;\n}\n");
        assert_eq!(b.cursor, Pos::new(1, 14));
        b.undo();
        assert_eq!(b.text(), "fn main() {\n}\n");
        assert_eq!(b.cursor, Pos::new(0, 11));
        b.redo();
        assert_eq!(b.text(), "fn main() {\n    let x = 1;\n}\n");
    }

    #[test]
    fn typing_coalesces_into_one_undo() {
        let mut b = buf("");
        for c in ["a", "b", "c"] {
            b.insert(c, EditKind::Typing);
        }
        assert_eq!(b.text(), "abc");
        b.undo();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn replace_multiline_selection() {
        let mut b = buf("one\ntwo\nthree");
        b.anchor = Pos::new(0, 1);
        b.cursor = Pos::new(2, 2);
        b.insert("X", EditKind::Other);
        assert_eq!(b.text(), "oXree");
        b.undo();
        assert_eq!(b.text(), "one\ntwo\nthree");
        assert_eq!((b.anchor, b.cursor), (Pos::new(0, 1), Pos::new(2, 2)));
    }

    #[test]
    fn unicode_columns() {
        let mut b = buf("héllo — wörld");
        b.move_to(Pos::new(0, 8), false);
        b.insert("ß", EditKind::Other);
        assert_eq!(b.text(), "héllo — ßwörld");
        b.backspace();
        assert_eq!(b.text(), "héllo — wörld");
    }

    #[test]
    fn newline_opens_block() {
        let mut b = buf("fn f() {}");
        b.move_to(Pos::new(0, 8), false);
        b.newline();
        assert_eq!(b.text(), "fn f() {\n    \n}");
        assert_eq!(b.cursor, Pos::new(1, 4));
    }

    #[test]
    fn newline_keeps_indent() {
        let mut b = buf("    let a = 1;");
        b.move_end(false);
        b.newline();
        assert_eq!(b.text(), "    let a = 1;\n    ");
    }

    #[test]
    fn backspace_eats_indent_level() {
        let mut b = buf("    a\n        x");
        b.move_to(Pos::new(1, 8), false);
        b.backspace();
        assert_eq!(b.lines[1], "    x");
        b.move_to(Pos::new(1, 3), false);
        b.backspace();
        assert_eq!(b.lines[1], " x");
    }

    #[test]
    fn comment_toggle() {
        let mut b = buf("fn f() {\n    a();\n\n    b();\n}");
        b.anchor = Pos::new(1, 0);
        b.cursor = Pos::new(3, 4);
        b.toggle_comment("//");
        assert_eq!(b.text(), "fn f() {\n    // a();\n\n    // b();\n}");
        b.toggle_comment("//");
        assert_eq!(b.text(), "fn f() {\n    a();\n\n    b();\n}");
    }

    #[test]
    fn indent_and_outdent_selection() {
        let mut b = buf("a\nb\nc");
        b.anchor = Pos::new(0, 0);
        b.cursor = Pos::new(2, 0);
        b.tab();
        assert_eq!(b.text(), "    a\n    b\nc");
        b.outdent_lines();
        assert_eq!(b.text(), "a\nb\nc");
    }

    #[test]
    fn word_motion() {
        let mut b = buf("let foo_bar = baz();");
        b.move_word_right(false);
        assert_eq!(b.cursor.col, 3);
        b.move_word_right(false);
        assert_eq!(b.cursor.col, 11);
        b.move_word_left(false);
        assert_eq!(b.cursor.col, 4);
        assert_eq!(b.word_at(Pos::new(0, 6)), (Pos::new(0, 4), Pos::new(0, 11)));
    }

    #[test]
    fn delete_and_duplicate_line() {
        let mut b = buf("a\nb\nc");
        b.move_to(Pos::new(1, 0), false);
        b.duplicate_line();
        assert_eq!(b.text(), "a\nb\nb\nc");
        assert_eq!(b.cursor.line, 2);
        b.delete_line();
        b.delete_line();
        assert_eq!(b.text(), "a\nb");
        b.move_to(Pos::new(1, 0), false);
        b.delete_line();
        assert_eq!(b.text(), "a");
    }

    #[test]
    fn replace_ranges_is_one_undo_step() {
        let mut b = buf("let a = a + a;\nfn a() {}\n");
        let ranges = [
            (Pos::new(0, 4), Pos::new(0, 5)),
            (Pos::new(0, 8), Pos::new(0, 9)),
            (Pos::new(0, 12), Pos::new(0, 13)),
            (Pos::new(1, 3), Pos::new(1, 4)),
        ];
        b.replace_ranges(&ranges, "total");
        assert_eq!(b.text(), "let total = total + total;\nfn total() {}\n");
        b.undo();
        assert_eq!(b.text(), "let a = a + a;\nfn a() {}\n");
        b.redo();
        assert_eq!(b.text(), "let total = total + total;\nfn total() {}\n");
    }

    #[test]
    fn crlf_preserved() {
        let b = buf("a\r\nb\r\n");
        assert_eq!(b.lines, vec!["a", "b", ""]);
        assert_eq!(b.text(), "a\r\nb\r\n");
    }

    #[test]
    fn changes_describe_edits() {
        let mut b = buf("one\ntwo\nthree");
        b.anchor = Pos::new(0, 1);
        b.cursor = Pos::new(1, 1);
        b.insert("a\nb\nc", EditKind::Other);
        let c = b.changes.last().unwrap();
        assert_eq!((c.line, c.old, c.new), (0, 2, 3));
        assert_eq!(b.lines.len(), 4);
    }

    #[test]
    fn detects_indent() {
        assert_eq!(buf("a:\n  b\n    c").indent, Indent::Spaces(2));
        assert_eq!(buf("fn a() {\n\tb\n}").indent, Indent::Tabs);
        assert_eq!(buf("fn a() {\n    b\n}").indent, Indent::Spaces(4));
    }
}
