//! The editing surface: gutter, text, caret, selection and the minimap.
//! Everything here is painted by hand; egui only supplies input, fonts
//! and a place to draw.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    epaint::Mesh, pos2, text::CCursor, text::LayoutJob, vec2, Align2, Color32, CursorIcon,
    Event, EventFilter, FontId, Galley, Id, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, TextFormat, Ui, Vec2,
};
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;

use crate::buffer::{Buffer, EditKind, Pos};
use crate::highlight::{comment_prefix, Highlighter, Span};
use crate::theme::Palette;

pub const FONT_SIZE: f32 = 13.0;
const TEXT_PAD: f32 = 4.0;
const MINIMAP_WIDTH: f32 = 116.0;
/// Height of one source line in the minimap, and of the bar drawn in it.
const MINI_ROW: f32 = 3.0;
const MINI_BAR: f32 = 2.0;
/// Width of one source column in the minimap.
const MINI_COL: f32 = 1.2;
/// Time per frame the highlighter may spend catching up.
const HIGHLIGHT_BUDGET: Duration = Duration::from_millis(6);

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Reveal {
    #[default]
    No,
    /// Scroll just enough to bring the caret into view.
    Nearest,
    /// Put the caret's line mid-screen (after a jump: find, go to line).
    Center,
}

#[derive(Default)]
pub struct View {
    pub scroll: Vec2,
    pub reveal: Reveal,
    clicks: u8,
    last_press: f64,
    last_press_at: Pos,
    blink_from: f64,
}

pub struct Env<'a> {
    pub pal: &'a Palette,
    pub ss: &'a SyntaxSet,
    pub theme: &'a Theme,
    /// Outline every occurrence of this.
    pub find: Option<&'a Query>,
    pub grab_focus: bool,
    /// False while a dialog is up: keep focus but ignore the keyboard.
    pub enabled: bool,
}

/// What to search for, and how strictly.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Query {
    pub text: String,
    pub case: bool,
    /// Only match when not glued to other letters, digits or `_`.
    pub word: bool,
}

impl Query {
    /// Occurrences in `line`, as char-column ranges, left to right and
    /// never overlapping.
    pub fn matches(&self, line: &str) -> Vec<(usize, usize)> {
        if self.text.is_empty() {
            return Vec::new();
        }
        // ASCII-only folding keeps byte offsets identical to the original.
        let (hay, needle) = if self.case {
            (line.to_string(), self.text.clone())
        } else {
            (line.to_ascii_lowercase(), self.text.to_ascii_lowercase())
        };
        let width = self.text.chars().count();
        let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        hay.match_indices(&needle)
            .filter(|(at, m)| {
                !self.word
                    || !(wordy(line[..*at].chars().next_back()) || wordy(line[at + m.len()..].chars().next()))
            })
            .map(|(at, _)| {
                let col = line[..at].chars().count();
                (col, col + width)
            })
            .collect()
    }
}

fn layout(ui: &Ui, text: &str, spans: &[Span], font: &FontId, fg: Color32) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let plain = |color, italics| TextFormat {
        font_id: font.clone(),
        color,
        italics,
        ..Default::default()
    };
    let covered: usize = spans.iter().map(|s| s.len as usize).sum();
    let mut ok = covered == text.len() && !spans.is_empty();
    if ok {
        let mut at = 0;
        for s in spans {
            match text.get(at..at + s.len as usize) {
                Some(piece) => job.append(piece, 0.0, plain(s.color, s.italic)),
                None => {
                    ok = false;
                    break;
                }
            }
            at += s.len as usize;
        }
    }
    if !ok {
        // Not highlighted yet (or mid-edit): draw it plain for now.
        job = LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        job.append(text, 0.0, plain(fg, false));
    }
    ui.fonts(|f| f.layout_job(job))
}

fn handle_key(buf: &mut Buffer, syntax: &str, key: Key, m: Modifiers, page: isize) -> bool {
    let mac = cfg!(target_os = "macos");
    let shift = m.shift;
    let word = if mac { m.alt } else { m.ctrl };
    let line_mod = mac && m.mac_cmd;
    match key {
        Key::ArrowLeft if line_mod => buf.move_home(shift),
        Key::ArrowLeft if word => buf.move_word_left(shift),
        Key::ArrowLeft => buf.move_left(shift),
        Key::ArrowRight if line_mod => buf.move_end(shift),
        Key::ArrowRight if word => buf.move_word_right(shift),
        Key::ArrowRight => buf.move_right(shift),
        Key::ArrowUp if line_mod => buf.move_doc_start(shift),
        Key::ArrowUp => buf.move_lines(-1, shift),
        Key::ArrowDown if line_mod => buf.move_doc_end(shift),
        Key::ArrowDown => buf.move_lines(1, shift),
        Key::Home if m.command => buf.move_doc_start(shift),
        Key::Home => buf.move_home(shift),
        Key::End if m.command => buf.move_doc_end(shift),
        Key::End => buf.move_end(shift),
        Key::PageUp => buf.move_lines(-page, shift),
        Key::PageDown => buf.move_lines(page, shift),
        Key::Backspace if line_mod => buf.delete_to_line_start(),
        Key::Backspace if word => buf.delete_word_back(),
        Key::Backspace => buf.backspace(),
        Key::Delete if word => buf.delete_word_forward(),
        Key::Delete => buf.delete_forward(),
        Key::Enter if m.command => {
            buf.move_end(false);
            buf.newline();
        }
        Key::Enter => buf.newline(),
        Key::Tab if shift => buf.outdent_lines(),
        Key::Tab => buf.tab(),
        Key::Escape => buf.move_to(buf.cursor, false),
        Key::A if m.command => buf.select_all(),
        Key::Z if m.command && shift => buf.redo(),
        Key::Z if m.command => buf.undo(),
        Key::Y if m.command => buf.redo(),
        Key::L if m.command => buf.select_line(buf.cursor.line),
        Key::D if m.command && shift => buf.duplicate_line(),
        Key::K if m.command && shift => buf.delete_line(),
        Key::CloseBracket if m.command => buf.indent_lines(),
        Key::OpenBracket if m.command => buf.outdent_lines(),
        Key::Slash if m.command => {
            if let Some(prefix) = comment_prefix(syntax) {
                buf.toggle_comment(prefix);
            }
        }
        _ => return false,
    }
    true
}

pub fn show(ui: &mut Ui, buf: &mut Buffer, hl: &mut Highlighter, view: &mut View, env: &Env) {
    let pal = env.pal;
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, Sense::hover());

    let font = FontId::monospace(FONT_SIZE);
    let (char_w, glyph_h) = ui.fonts(|f| (f.glyph_width(&font, 'm'), f.row_height(&font)));
    let row_h = (glyph_h * 1.2).round();
    let text_dy = ((row_h - glyph_h) / 2.0).floor();

    let digits = buf.lines.len().to_string().len().max(3) as f32;
    let mini_w = MINIMAP_WIDTH.min(rect.width() * 0.2);
    let gutter_right = rect.left() + (digits + 3.0) * char_w;
    let minimap = Rect::from_min_max(pos2(rect.right() - mini_w, rect.top()), rect.max);
    let main = Rect::from_min_max(rect.min, pos2(minimap.left(), rect.bottom()));
    let text_rect = Rect::from_min_max(pos2(gutter_right, rect.top()), main.max);
    let page = ((text_rect.height() / row_h) as isize - 1).max(1);

    let id = Id::new("slate-editor");
    let resp = ui.interact(main, id, Sense::click_and_drag());
    let mini_resp = ui.interact(minimap, id.with("minimap"), Sense::click_and_drag());

    let now = ui.input(|i| i.time);

    // ------------------------------------------------------------ mouse
    let pos_at = |ui: &Ui, buf: &Buffer, hl: &Highlighter, scroll: Vec2, p: Pos2| -> Pos {
        let row = ((p.y - text_rect.top() + scroll.y) / row_h).floor().max(0.0) as usize;
        let line = row.min(buf.lines.len() - 1);
        let galley = layout(ui, &buf.lines[line], hl.spans(line), &font, pal.fg);
        let x = p.x - text_rect.left() - TEXT_PAD + scroll.x;
        let col = galley.cursor_from_pos(vec2(x, 0.0)).index;
        Pos::new(line, col.min(buf.line_len(line)))
    };

    let (pressed, pointer, mods) = ui.input(|i| {
        (i.pointer.primary_pressed(), i.pointer.interact_pos(), i.modifiers)
    });
    if let (true, true, Some(p)) = (pressed, resp.contains_pointer(), pointer) {
        ui.memory_mut(|m| m.request_focus(id));
        let at = pos_at(ui, buf, hl, view.scroll, p);
        let repeat = now - view.last_press < 0.4 && at.line == view.last_press_at.line;
        view.clicks = if repeat { view.clicks % 3 + 1 } else { 1 };
        view.last_press = now;
        view.last_press_at = at;
        if p.x < gutter_right || view.clicks == 3 {
            buf.select_line(at.line);
        } else if view.clicks == 2 {
            let (a, b) = buf.word_at(at);
            buf.move_to(a, false);
            buf.move_to(b, true);
        } else {
            buf.move_to(at, mods.shift);
        }
        view.blink_from = now;
    } else if let (true, 1, Some(p)) = (resp.dragged(), view.clicks, pointer) {
        let at = pos_at(ui, buf, hl, view.scroll, p);
        buf.move_to(at, true);
        // Dragging past the top or bottom edge scrolls.
        let beyond = (p.y - text_rect.bottom()).max(0.0) + (p.y - text_rect.top()).min(0.0);
        if beyond != 0.0 {
            view.scroll.y += beyond * 0.25;
            ui.ctx().request_repaint();
        }
    }
    if resp.hovered() && pointer.is_some_and(|p| p.x >= gutter_right) {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
    }

    // --------------------------------------------------------- keyboard
    if env.grab_focus {
        ui.memory_mut(|m| m.request_focus(id));
    }
    let focused = env.enabled && ui.memory(|m| m.has_focus(id));
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let acted = match event {
                Event::Text(t) => {
                    if t.chars().all(|c| !c.is_control()) {
                        buf.insert(&t, EditKind::Typing);
                    }
                    true
                }
                Event::Paste(t) => {
                    buf.insert(&t, EditKind::Other);
                    true
                }
                Event::Copy => {
                    ui.ctx().copy_text(buf.copy());
                    false
                }
                Event::Cut => {
                    let text = buf.cut();
                    ui.ctx().copy_text(text);
                    true
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => handle_key(buf, &hl.syntax, key, modifiers, page),
                _ => false,
            };
            if acted {
                view.reveal = Reveal::Nearest;
                view.blink_from = now;
            }
        }
    }

    // ------------------------------------------------------- highlighting
    for change in buf.changes.drain(..) {
        hl.apply(&change);
    }
    if hl.run(&buf.lines, env.ss, env.theme, HIGHLIGHT_BUDGET) {
        ui.ctx().request_repaint();
    }

    // ----------------------------------------------------------- scrolling
    let lines = buf.lines.len();
    let max_scroll = (lines - 1) as f32 * row_h;
    let mini_total = lines as f32 * MINI_ROW;
    let mini_overflow = (mini_total - minimap.height()).max(0.0);

    if resp.contains_pointer() || mini_resp.contains_pointer() {
        let delta = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta));
        view.scroll -= delta;
    }
    if mini_resp.is_pointer_button_down_on() {
        if let Some(p) = pointer {
            // Put the middle of the viewport marker under the pointer. The
            // minimap itself slides as the view scrolls, hence the algebra.
            let k = MINI_ROW / row_h;
            let slide = if max_scroll > 0.0 { mini_overflow / max_scroll } else { 0.0 };
            let y = p.y - minimap.top() - text_rect.height() * k / 2.0;
            view.scroll.y = y / (k - slide).max(1e-4);
        }
    }
    if view.reveal != Reveal::No {
        let c = buf.cursor;
        let y = c.line as f32 * row_h;
        let h = text_rect.height();
        if view.reveal == Reveal::Center {
            if y < view.scroll.y + row_h || y + 2.0 * row_h > view.scroll.y + h {
                view.scroll.y = y - h / 2.0;
            }
        } else if y < view.scroll.y {
            view.scroll.y = y;
        } else if y + row_h > view.scroll.y + h {
            view.scroll.y = y + row_h - h;
        }
        let galley = layout(ui, &buf.lines[c.line], hl.spans(c.line), &font, pal.fg);
        let x = galley.pos_from_cursor(CCursor::new(c.col)).min.x;
        let w = text_rect.width() - TEXT_PAD;
        let margin = (char_w * 6.0).min(w / 4.0);
        if x < view.scroll.x + margin {
            view.scroll.x = x - margin;
        } else if x > view.scroll.x + w - margin {
            view.scroll.x = x - w + margin;
        }
        view.reveal = Reveal::No;
    }
    let first = ((view.scroll.y.max(0.0) / row_h) as usize).min(lines - 1);
    let last = (first + (text_rect.height() / row_h).ceil() as usize + 1).min(lines);
    let widest = buf.lines[first..last]
        .iter()
        .map(|l| l.len() + 3 * l.matches('\t').count())
        .max()
        .unwrap_or(0) as f32
        * char_w;
    let max_x = (widest - text_rect.width() + 8.0 * char_w).max(0.0);
    view.scroll.y = view.scroll.y.clamp(0.0, max_scroll).round();
    view.scroll.x = view.scroll.x.clamp(0.0, max_x).round();
    let scroll = view.scroll;

    // ------------------------------------------------------------ painting
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.bg);
    let text_painter = ui.painter_at(text_rect);
    let origin_x = text_rect.left() + TEXT_PAD - scroll.x;
    let selection = buf.selection();
    let caret_on = focused && ((now - view.blink_from) % 1.0) < 0.55;
    if focused {
        ui.ctx().request_repaint_after(Duration::from_millis(275));
    }

    let first = ((scroll.y / row_h) as usize).min(lines - 1);
    let last = (first + (text_rect.height() / row_h).ceil() as usize + 1).min(lines);
    for i in first..last {
        let y = text_rect.top() + i as f32 * row_h - scroll.y;
        let galley = layout(ui, &buf.lines[i], hl.spans(i), &font, pal.fg);
        let x_of = |col: usize| origin_x + galley.pos_from_cursor(CCursor::new(col)).min.x;
        let current = i == buf.cursor.line;

        if current && selection.is_none() {
            let band = Rect::from_min_max(pos2(main.left(), y), pos2(main.right(), y + row_h));
            painter.rect_filled(band, 0.0, pal.line_highlight);
        }
        if let Some((a, b)) = selection {
            if (a.line..=b.line).contains(&i) {
                let x0 = if i == a.line { x_of(a.col) } else { origin_x };
                let x1 = if i == b.line {
                    x_of(b.col)
                } else {
                    // Show the line break as part of the selection.
                    origin_x + galley.size().x + char_w * 0.6
                };
                let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + row_h));
                text_painter.rect_filled(r, 2.0, pal.selection);
            }
        }
        if let Some(query) = env.find {
            for (a, b) in query.matches(&buf.lines[i]) {
                let r = Rect::from_min_max(pos2(x_of(a), y + 1.0), pos2(x_of(b), y + row_h - 1.0));
                text_painter.rect_filled(r, 2.0, pal.find.gamma_multiply(0.18));
                text_painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, pal.find), StrokeKind::Inside);
            }
        }
        if current && caret_on {
            let x = x_of(buf.cursor.col);
            let r = Rect::from_min_max(pos2(x - 1.0, y + 1.0), pos2(x + 1.0, y + row_h - 1.0));
            text_painter.rect_filled(r, 0.0, pal.caret);
        }
        text_painter.galley(pos2(origin_x, y + text_dy), galley, pal.fg);
        painter.text(
            pos2(gutter_right - char_w * 1.5, y + text_dy),
            Align2::RIGHT_TOP,
            (i + 1).to_string(),
            font.clone(),
            if current { pal.fg } else { pal.gutter_fg },
        );
    }

    // ------------------------------------------------------------- minimap
    let mini = ui.painter_at(minimap);
    let slide = if max_scroll > 0.0 {
        (scroll.y / max_scroll) * mini_overflow
    } else {
        0.0
    };
    let active = mini_resp.hovered() || mini_resp.is_pointer_button_down_on();
    let marker = Rect::from_min_size(
        pos2(minimap.left(), minimap.top() + scroll.y / row_h * MINI_ROW - slide),
        vec2(minimap.width(), text_rect.height() / row_h * MINI_ROW),
    );
    mini.rect_filled(marker, 0.0, pal.fg.gamma_multiply(if active { 0.16 } else { 0.09 }));

    let mut mesh = Mesh::default();
    let max_col = ((minimap.width() - 10.0) / MINI_COL) as usize;
    let tab = match buf.indent {
        crate::buffer::Indent::Spaces(n) => n,
        crate::buffer::Indent::Tabs => 4,
    };
    let m_first = (slide / MINI_ROW) as usize;
    let m_last = (((slide + minimap.height()) / MINI_ROW).ceil() as usize).min(lines);
    let whole = [Span { len: 0, color: pal.fg, italic: false }];
    for i in m_first..m_last {
        let line = &buf.lines[i];
        if line.is_empty() {
            continue;
        }
        let y = minimap.top() + i as f32 * MINI_ROW - slide;
        let mut bar = |from: usize, to: usize, color: Color32| {
            let r = Rect::from_min_max(
                pos2(minimap.left() + 5.0 + from as f32 * MINI_COL, y),
                pos2(minimap.left() + 5.0 + to as f32 * MINI_COL, y + MINI_BAR),
            );
            mesh.add_colored_rect(r, color.gamma_multiply(0.7));
        };
        let spans = hl.spans(i);
        let covered: usize = spans.iter().map(|s| s.len as usize).sum();
        let spans = if covered == line.len() { spans } else { &whole[..] };
        let (mut col, mut at) = (0usize, 0usize);
        'line: for s in spans {
            let len = if s.len == 0 { line.len() } else { s.len as usize };
            let Some(piece) = line.get(at..at + len) else { break };
            at += len;
            let mut run: Option<usize> = None;
            for c in piece.chars() {
                if col >= max_col {
                    if let Some(from) = run.take() {
                        bar(from, col, s.color);
                    }
                    break 'line;
                }
                if c.is_whitespace() {
                    if let Some(from) = run.take() {
                        bar(from, col, s.color);
                    }
                    col += if c == '\t' { tab } else { 1 };
                } else {
                    run.get_or_insert(col);
                    col += 1;
                }
            }
            if let Some(from) = run {
                bar(from, col.min(max_col), s.color);
            }
        }
    }
    mini.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::Query;

    #[test]
    fn query_matching() {
        let q = |text: &str, case, word| Query { text: text.into(), case, word };
        assert_eq!(q("ab", false, false).matches("AB cab Ab"), vec![(0, 2), (4, 6), (7, 9)]);
        assert_eq!(q("Ab", true, false).matches("AB cab Ab"), vec![(7, 9)]);
        assert_eq!(q("tier", false, true).matches("Tier tier_blue (tier) tiers"), vec![(0, 4), (16, 20)]);
        assert_eq!(q("ö", false, false).matches("höhö"), vec![(1, 2), (3, 4)]);
        assert!(q("", false, false).matches("anything").is_empty());
    }
}
