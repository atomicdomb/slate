//! Find in files: a background search over the project.
//!
//! The walk runs on its own thread and streams one message per file with
//! hits, so results appear as they are found and typing is never blocked.
//! Tabs with unsaved changes are searched as they are in the editor, not
//! as they are on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use eframe::egui::Context;

use crate::editor::Query;

/// Stop collecting after this many matches; nobody reads further.
const MAX_MATCHES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const SNIPPET_CHARS: usize = 300;

pub struct Hit {
    pub line: usize,
    /// Char columns of the first match on the line.
    pub start: usize,
    pub end: usize,
    /// The line, without leading whitespace and cut to a sane length.
    pub text: String,
}

pub struct FileHits {
    /// Relative to the project root.
    pub path: String,
    pub hits: Vec<Hit>,
    pub matches: usize,
    pub collapsed: bool,
}

enum Msg {
    File(FileHits),
    Done { files: usize, truncated: bool },
}

#[derive(Default)]
pub struct Search {
    pub open: bool,
    pub grab_focus: bool,
    /// What is in the search field.
    pub query: Query,
    /// What the results on screen were found with.
    pub ran: Query,
    pub results: Vec<FileHits>,
    pub matches: usize,
    pub files_searched: usize,
    pub running: bool,
    pub truncated: bool,
    rx: Option<Receiver<Msg>>,
    cancel: Arc<AtomicBool>,
}

fn search_text(path: &str, text: &str, query: &Query) -> Option<FileHits> {
    let mut hits = Vec::new();
    let mut matches = 0;
    for (line, content) in text.lines().enumerate() {
        let found = query.matches(content);
        let Some(&(start, end)) = found.first() else { continue };
        matches += found.len();
        hits.push(Hit {
            line,
            start,
            end,
            text: content.trim_start().chars().take(SNIPPET_CHARS).collect(),
        });
    }
    (!hits.is_empty()).then(|| FileHits { path: path.to_string(), hits, matches, collapsed: false })
}

fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return None; // binary
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

impl Search {
    /// Search `files` (relative to `root`) for the current query.
    /// `unsaved` holds the live text of modified tabs, by absolute path.
    pub fn start(&mut self, root: &Path, files: Vec<String>, unsaved: HashMap<PathBuf, String>, ctx: &Context) {
        self.stop();
        self.results.clear();
        self.matches = 0;
        self.files_searched = 0;
        self.truncated = false;
        self.ran = self.query.clone();
        if self.query.text.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.rx = Some(rx);
        self.cancel = cancel.clone();
        self.running = true;

        let (root, query, ctx) = (root.to_path_buf(), self.query.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut total = 0;
            let mut searched = 0;
            let mut truncated = false;
            for rel in files {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let path = root.join(&rel);
                let Some(text) = unsaved.get(&path).cloned().or_else(|| read_text(&path)) else { continue };
                searched += 1;
                if let Some(found) = search_text(&rel, &text, &query) {
                    total += found.matches;
                    if tx.send(Msg::File(found)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                    if total >= MAX_MATCHES {
                        truncated = true;
                        break;
                    }
                }
            }
            let _ = tx.send(Msg::Done { files: searched, truncated });
            ctx.request_repaint();
        });
    }

    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.rx = None;
        self.running = false;
    }

    /// Take in whatever the search thread has found since last frame.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::File(found) => {
                    self.matches += found.matches;
                    self.results.push(found);
                }
                Msg::Done { files, truncated } => {
                    self.files_searched = files;
                    self.truncated = truncated;
                    self.running = false;
                }
            }
        }
    }

    pub fn summary(&self) -> String {
        if self.ran.text.is_empty() {
            return String::new();
        }
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        if self.running {
            return format!("Searching… {}", plural(self.matches, "match").replace("matchs", "matches"));
        }
        if self.matches == 0 {
            return format!("No matches in {}", plural(self.files_searched, "file"));
        }
        let matches = plural(self.matches, "match").replace("matchs", "matches");
        let more = if self.truncated { " (stopped early, refine the search)" } else { "" };
        format!("{matches} in {}{more}", plural(self.results.len(), "file"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lines_and_counts_every_match() {
        let q = Query { text: "tier".into(), case: false, word: false };
        let text = "pub const TIER_BLUE: u32 = 1;\r\n\n    Tier::Blue => tier(),\n";
        let found = search_text("loot.rs", text, &q).unwrap();
        assert_eq!(found.matches, 3);
        assert_eq!(found.hits.len(), 2);
        assert_eq!((found.hits[0].line, found.hits[0].start, found.hits[0].end), (0, 10, 14));
        assert_eq!((found.hits[1].line, found.hits[1].start), (2, 4));
        assert_eq!(found.hits[1].text, "Tier::Blue => tier(),");
        assert!(search_text("x", "nothing here", &q).is_none());
    }
}
