use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};

use super::vim::{VimMode, emit_osc52, word_backward, word_end_forward, word_forward};
use crate::EventResult;
use crate::traits::{HandleEvents, IsFocus, SetFocus};

/// State of a multi-line [`CodeInputField`](crate::widgets::CodeInputField)
/// editor: line buffer, cursor (line + column), and scroll offsets.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct CodeInputFieldState {
    #[getset(get = "pub")]
    #[builder(default = "vec![String::new()]")]
    lines: Vec<String>,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    active_line: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    cursor_col: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    scroll_offset: usize,
    #[getset(get_copy = "pub")]
    #[builder(default = "0")]
    h_scroll: usize,
    // `set_focused` needs the format-on-blur side effect (see `impl SetFocus` below), so
    // getset's auto-generated setter is skipped here to avoid it shadowing the trait
    // method on direct calls; the getter is hand-written just below.
    #[getset(skip)]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(default = "false")]
    disabled: bool,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    placeholder: Option<String>,
    #[getset(get_copy = "pub")]
    #[builder(default = "Some(Duration::from_millis(300))")]
    space_indent: Option<Duration>,
    #[getset(skip)]
    #[builder(setter(skip), default)]
    last_space: Option<(Instant, usize, usize)>,
    #[getset(get_copy = "pub")]
    #[builder(default = "None")]
    language: Option<ferrowl_syntax::Language>,
    /// Enables vim-like modal editing (`Normal`/`Insert`/`Visual`). When
    /// `false`, [`handle_events`](HandleEvents::handle_events) behaves exactly
    /// like the plain single-mode editor this field started as.
    #[getset(get_copy = "pub")]
    #[builder(default = "true")]
    vim: bool,
    // Runtime vim state below is never builder-configurable: it's derived
    // purely from key events, so exposing it through the builder would let
    // callers construct inconsistent combinations (e.g. `Visual` with no
    // anchor). Access goes through the hand-written methods further down.
    #[getset(skip)]
    #[builder(setter(skip), default = "VimMode::Normal")]
    mode: VimMode,
    #[getset(skip)]
    #[builder(setter(skip), default)]
    visual_anchor: Option<(usize, usize)>,
    #[getset(skip)]
    #[builder(setter(skip), default)]
    register: Option<(String, bool)>,
    #[getset(skip)]
    #[builder(setter(skip), default)]
    undo: Option<(Vec<String>, usize, usize)>,
    // Holds a lone `g`/`d`/`y` waiting for its matching second press (`gg`,
    // `dd`, `yy`); any other key clears it before being handled itself.
    #[getset(skip)]
    #[builder(setter(skip), default)]
    pending: Option<char>,
}

impl CodeInputFieldState {
    /// Whether the field currently holds focus.
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Returns the full text with lines joined by `\n`.
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Replaces the full text, resetting scroll and placing the cursor at
    /// the end of the last line.
    pub fn set_content(&mut self, s: &str) {
        self.lines = s.split('\n').map(|l| l.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        // Place cursor at end of last line so Backspace works immediately.
        self.active_line = self.lines.len() - 1;
        self.cursor_col = self.lines[self.active_line].chars().count();
        self.scroll_offset = 0;
        self.h_scroll = 0;
        if self.vim {
            // Fresh content means fresh history: an undo snapshot pointing at
            // the previous text no longer means anything useful.
            self.mode = VimMode::Normal;
            self.visual_anchor = None;
            self.undo = None;
            self.pending = None;
            self.clamp_normal();
        }
    }

    /// The current vim mode. Always [`VimMode::Normal`] when `vim` is disabled.
    pub fn vim_mode(&self) -> VimMode {
        self.mode
    }

    /// Status-line label for the current mode, or `None` when vim mode is off
    /// or the field isn't focused (nothing to show).
    pub fn mode_label(&self) -> Option<&'static str> {
        if !self.vim || !self.focused {
            return None;
        }
        Some(match self.mode {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::Visual { linewise: false } => "VISUAL",
            VimMode::Visual { linewise: true } => "V-LINE",
        })
    }

    /// Normalized, inclusive `(start, end)` document-order span of the
    /// current Visual selection, or `None` outside Visual mode. Linewise
    /// selections span the full width of every covered line.
    pub fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.visual_anchor?;
        let VimMode::Visual { linewise } = self.mode else {
            return None;
        };
        let cursor = (self.active_line, self.cursor_col);
        let (start, end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        if linewise {
            let last_col = self.lines[end.0].chars().count().saturating_sub(1);
            Some(((start.0, 0), (end.0, last_col)))
        } else {
            Some((start, end))
        }
    }

    fn clamp_cursor(&mut self) {
        let line_len = self.lines[self.active_line].chars().count();
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }

    /// Vim's block-cursor rule: the cursor always sits ON a character, so it
    /// clamps to the last valid index (`len - 1`), not `len` — an empty line
    /// clamps to `0`.
    fn clamp_normal(&mut self) {
        let max = self.lines[self.active_line]
            .chars()
            .count()
            .saturating_sub(1);
        if self.cursor_col > max {
            self.cursor_col = max;
        }
    }

    fn snapshot_undo(&mut self) {
        self.undo = Some((self.lines.clone(), self.active_line, self.cursor_col));
    }

    fn set_register(&mut self, text: String, linewise: bool) {
        emit_osc52(&text);
        self.register = Some((text, linewise));
    }

    /// Extracts the inclusive charwise span `start..=end` (document order) as
    /// a single string, joining crossed lines with `\n`.
    fn extract_charwise(&self, start: (usize, usize), end: (usize, usize)) -> String {
        if start.0 == end.0 {
            let chars: Vec<char> = self.lines[start.0].chars().collect();
            let to = (end.1 + 1).min(chars.len());
            let from = start.1.min(to);
            chars[from..to].iter().collect()
        } else {
            let mut parts = Vec::new();
            let first: Vec<char> = self.lines[start.0].chars().collect();
            let from = start.1.min(first.len());
            parts.push(first[from..].iter().collect::<String>());
            for l in start.0 + 1..end.0 {
                parts.push(self.lines[l].clone());
            }
            let last: Vec<char> = self.lines[end.0].chars().collect();
            let to = (end.1 + 1).min(last.len());
            parts.push(last[..to].iter().collect::<String>());
            parts.join("\n")
        }
    }

    fn yank_current_line(&mut self) {
        let text = self.lines[self.active_line].clone();
        self.set_register(text, true);
    }

    fn yank_selection(&mut self) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let linewise = matches!(self.mode, VimMode::Visual { linewise: true });
        let text = if linewise {
            self.lines[start.0..=end.0].join("\n")
        } else {
            self.extract_charwise(start, end)
        };
        self.set_register(text, linewise);
    }

    fn delete_current_line(&mut self) {
        if self.disabled {
            return;
        }
        self.snapshot_undo();
        let text = self.lines[self.active_line].clone();
        self.set_register(text, true);
        if self.lines.len() == 1 {
            self.lines[0] = String::new();
        } else {
            self.lines.remove(self.active_line);
            if self.active_line >= self.lines.len() {
                self.active_line = self.lines.len() - 1;
            }
        }
        self.cursor_col = 0;
        self.clamp_normal();
    }

    fn delete_selection(&mut self) {
        if self.disabled {
            return;
        }
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let linewise = matches!(self.mode, VimMode::Visual { linewise: true });
        self.snapshot_undo();
        let text = if linewise {
            self.lines[start.0..=end.0].join("\n")
        } else {
            self.extract_charwise(start, end)
        };
        self.set_register(text, linewise);
        if linewise {
            if start.0 == 0 && end.0 == self.lines.len() - 1 {
                self.lines = vec![String::new()];
                self.active_line = 0;
            } else {
                self.lines.drain(start.0..=end.0);
                self.active_line = start.0.min(self.lines.len() - 1);
            }
            self.cursor_col = 0;
        } else {
            let first: Vec<char> = self.lines[start.0].chars().collect();
            let last: Vec<char> = self.lines[end.0].chars().collect();
            let to = (end.1 + 1).min(last.len());
            let mut joined: Vec<char> = first[..start.1.min(first.len())].to_vec();
            joined.extend_from_slice(&last[to..]);
            self.lines
                .splice(start.0..=end.0, [joined.into_iter().collect()]);
            self.active_line = start.0;
            self.cursor_col = start.1;
        }
        self.mode = VimMode::Normal;
        self.visual_anchor = None;
        self.clamp_normal();
    }

    /// Places the cursor at the selection start and returns to Normal mode;
    /// used after `y` (delete uses its own line/col bookkeeping instead).
    fn enter_normal_at_selection_start(&mut self) {
        if let Some((start, _)) = self.selection_range() {
            self.active_line = start.0;
            self.cursor_col = start.1;
        }
        self.mode = VimMode::Normal;
        self.visual_anchor = None;
        self.clamp_normal();
    }

    fn paste(&mut self, after: bool) {
        let Some((text, linewise)) = self.register.clone() else {
            return;
        };
        self.snapshot_undo();
        if linewise {
            let new_lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
            let insert_at = if after {
                self.active_line + 1
            } else {
                self.active_line
            };
            for (i, l) in new_lines.into_iter().enumerate() {
                self.lines.insert(insert_at + i, l);
            }
            self.active_line = insert_at;
            self.cursor_col = 0;
        } else {
            let parts: Vec<&str> = text.split('\n').collect();
            let chars: Vec<char> = self.lines[self.active_line].chars().collect();
            let insert_col = if after {
                (self.cursor_col + 1).min(chars.len())
            } else {
                self.cursor_col
            };
            if parts.len() == 1 {
                let insert_chars: Vec<char> = parts[0].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + insert_chars.len());
                new_chars.extend_from_slice(&chars[..insert_col]);
                new_chars.extend_from_slice(&insert_chars);
                new_chars.extend_from_slice(&chars[insert_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col = insert_col + insert_chars.len().saturating_sub(1);
            } else {
                let before: String = chars[..insert_col].iter().collect();
                let tail: String = chars[insert_col..].iter().collect();
                let first_line = format!("{before}{}", parts[0]);
                let last_line = format!("{}{tail}", parts[parts.len() - 1]);
                let mut new_lines = vec![first_line];
                new_lines.extend(parts[1..parts.len() - 1].iter().map(|s| s.to_string()));
                new_lines.push(last_line);
                let last_idx = self.active_line + new_lines.len() - 1;
                let last_col = parts[parts.len() - 1].chars().count().saturating_sub(1);
                self.lines
                    .splice(self.active_line..=self.active_line, new_lines);
                self.active_line = last_idx;
                self.cursor_col = last_col;
            }
        }
        self.clamp_normal();
    }

    /// `o`: open a new, auto-indented line below the current one (same
    /// indent rule Enter uses when splitting at end-of-line).
    fn open_line_below(&mut self) {
        let line = self.lines[self.active_line].clone();
        let indent = match self.language {
            Some(lang) => {
                let lead = line.chars().take_while(|c| *c == ' ').count() as i32;
                let delta = ferrowl_syntax::indent_delta(lang, &line);
                (lead + 4 * delta).max(0) as usize
            }
            None => 0,
        };
        self.active_line += 1;
        self.lines.insert(self.active_line, " ".repeat(indent));
        self.cursor_col = indent;
    }

    /// `O`: open a new line above the current one, copying its indent as-is.
    fn open_line_above(&mut self) {
        let indent = self.lines[self.active_line]
            .chars()
            .take_while(|c| *c == ' ')
            .count();
        self.lines.insert(self.active_line, " ".repeat(indent));
        self.cursor_col = indent;
    }

    /// Applies a single motion key to `(active_line, cursor_col)`, shared by
    /// Normal and Visual mode. Returns whether `code` was a recognized
    /// motion. Arrow keys keep the legacy wrap-to-adjacent-line behavior;
    /// `h`/`l` are vim-style and never wrap.
    fn apply_motion(&mut self, modifiers: KeyModifiers, code: KeyCode) -> bool {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Up) => {
                self.active_line = self.active_line.saturating_sub(1);
                self.clamp_normal();
                true
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                }
                self.clamp_normal();
                true
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.active_line > 0 {
                    self.active_line -= 1;
                    self.cursor_col = self.lines[self.active_line].chars().count();
                }
                self.clamp_normal();
                true
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let line_len = self.lines[self.active_line].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                    self.cursor_col = 0;
                }
                self.clamp_normal();
                true
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => match c {
                'h' => {
                    self.cursor_col = self.cursor_col.saturating_sub(1);
                    true
                }
                'l' => {
                    self.cursor_col += 1;
                    self.clamp_normal();
                    true
                }
                'j' => {
                    if self.active_line + 1 < self.lines.len() {
                        self.active_line += 1;
                    }
                    self.clamp_normal();
                    true
                }
                'k' => {
                    self.active_line = self.active_line.saturating_sub(1);
                    self.clamp_normal();
                    true
                }
                '0' => {
                    self.cursor_col = 0;
                    true
                }
                '$' => {
                    self.cursor_col = self.lines[self.active_line]
                        .chars()
                        .count()
                        .saturating_sub(1);
                    true
                }
                'w' => {
                    let (l, c) = word_forward(&self.lines, self.active_line, self.cursor_col);
                    self.active_line = l;
                    self.cursor_col = c;
                    self.clamp_normal();
                    true
                }
                'e' => {
                    let (l, c) = word_end_forward(&self.lines, self.active_line, self.cursor_col);
                    self.active_line = l;
                    self.cursor_col = c;
                    true
                }
                'b' => {
                    let (l, c) = word_backward(&self.lines, self.active_line, self.cursor_col);
                    self.active_line = l;
                    self.cursor_col = c;
                    true
                }
                'G' => {
                    self.active_line = self.lines.len() - 1;
                    self.cursor_col = 0;
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn handle_normal_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        // `gg`/`dd`/`yy` chords: a matching second press consumes `pending`
        // and fires; any other key clears it before falling through.
        if let (KeyModifiers::NONE, KeyCode::Char(c @ ('g' | 'd' | 'y'))) = (modifiers, code) {
            if self.pending == Some(c) {
                self.pending = None;
                match c {
                    'g' => {
                        self.active_line = 0;
                        self.cursor_col = 0;
                        self.clamp_normal();
                    }
                    'd' => self.delete_current_line(),
                    'y' => self.yank_current_line(),
                    _ => unreachable!(),
                }
            } else {
                self.pending = Some(c);
            }
            return EventResult::Consumed;
        }
        self.pending = None;

        if self.apply_motion(modifiers, code) {
            return EventResult::Consumed;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Char('x')) => {
                if !self.disabled {
                    let len = self.lines[self.active_line].chars().count();
                    if len > 0 {
                        self.snapshot_undo();
                        let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                        let removed = chars[self.cursor_col];
                        let new_line: String = chars
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != self.cursor_col)
                            .map(|(_, c)| *c)
                            .collect();
                        self.lines[self.active_line] = new_line;
                        self.set_register(removed.to_string(), false);
                        self.clamp_normal();
                    }
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('p')) => {
                if !self.disabled {
                    self.paste(true);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('P')) => {
                if !self.disabled {
                    self.paste(false);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('u')) => {
                if !self.disabled
                    && let Some((lines, active_line, cursor_col)) = self.undo.take()
                {
                    let cur = (self.lines.clone(), self.active_line, self.cursor_col);
                    self.lines = lines;
                    self.active_line = active_line;
                    self.cursor_col = cursor_col;
                    self.undo = Some(cur);
                    self.clamp_normal();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('i')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('a')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    let len = self.lines[self.active_line].chars().count();
                    self.cursor_col = (self.cursor_col + 1).min(len);
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('I')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    self.cursor_col = self.lines[self.active_line]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('A')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    self.cursor_col = self.lines[self.active_line].chars().count();
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('o')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    self.open_line_below();
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('O')) => {
                if !self.disabled {
                    self.snapshot_undo();
                    self.open_line_above();
                    self.mode = VimMode::Insert;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('v')) => {
                self.visual_anchor = Some((self.active_line, self.cursor_col));
                self.mode = VimMode::Visual { linewise: false };
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('V')) => {
                self.visual_anchor = Some((self.active_line, self.cursor_col));
                self.mode = VimMode::Visual { linewise: true };
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Esc) => EventResult::Unhandled(modifiers, code),
            (KeyModifiers::NONE, KeyCode::Tab) => EventResult::Unhandled(modifiers, code),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                EventResult::Unhandled(modifiers, code)
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(_)) => EventResult::Consumed,
            (m, c) => EventResult::Unhandled(m, c),
        }
    }

    fn handle_visual_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if modifiers == KeyModifiers::NONE && code == KeyCode::Char('g') {
            if self.pending == Some('g') {
                self.pending = None;
                self.active_line = 0;
                self.cursor_col = 0;
                self.clamp_normal();
            } else {
                self.pending = Some('g');
            }
            return EventResult::Consumed;
        }
        self.pending = None;

        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.mode = VimMode::Normal;
                self.visual_anchor = None;
                self.clamp_normal();
                return EventResult::Consumed;
            }
            (KeyModifiers::NONE, KeyCode::Char('v')) => {
                self.mode = VimMode::Normal;
                self.visual_anchor = None;
                self.clamp_normal();
                return EventResult::Consumed;
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('V')) => {
                self.mode = VimMode::Visual { linewise: true };
                return EventResult::Consumed;
            }
            (KeyModifiers::NONE, KeyCode::Char('y')) => {
                self.yank_selection();
                self.enter_normal_at_selection_start();
                return EventResult::Consumed;
            }
            (KeyModifiers::NONE, KeyCode::Char('d') | KeyCode::Char('x')) => {
                self.delete_selection();
                return EventResult::Consumed;
            }
            _ => {}
        }

        if self.apply_motion(modifiers, code) {
            return EventResult::Consumed;
        }

        match (modifiers, code) {
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(_)) => EventResult::Consumed,
            (m, c) => EventResult::Unhandled(m, c),
        }
    }

    fn handle_insert_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Esc) => {
                self.last_space = None;
                self.mode = VimMode::Normal;
                let max = self.lines[self.active_line]
                    .chars()
                    .count()
                    .saturating_sub(1);
                self.cursor_col = self.cursor_col.saturating_sub(1).min(max);
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.last_space = None;
                let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + 4);
                new_chars.extend_from_slice(&chars[..self.cursor_col]);
                new_chars.extend([' ', ' ', ' ', ' ']);
                new_chars.extend_from_slice(&chars[self.cursor_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col += 4;
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.last_space = None;
                let leading = self.lines[self.active_line]
                    .chars()
                    .take_while(|c| *c == ' ')
                    .count()
                    .min(4);
                if leading > 0 {
                    self.lines[self.active_line] =
                        self.lines[self.active_line].chars().skip(leading).collect();
                    self.cursor_col = self.cursor_col.saturating_sub(leading);
                }
                EventResult::Consumed
            }
            _ => self.handle_edit_key(modifiers, code),
        }
    }
}

impl SetFocus for CodeInputFieldState {
    fn set_focused(&mut self, focus: bool) {
        if self.focused
            && !focus
            && !self.disabled
            && let Some(lang) = self.language
            && let Some(new) = ferrowl_syntax::format(lang, &self.content())
            && new != self.content()
        {
            self.set_content(&new);
        }
        if !focus {
            self.mode = VimMode::Normal;
            self.visual_anchor = None;
            self.pending = None;
        }
        self.focused = focus;
    }
}

impl IsFocus for CodeInputFieldState {
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl HandleEvents for CodeInputFieldState {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if !self.vim {
            return self.handle_edit_key(modifiers, code);
        }
        match self.mode {
            VimMode::Insert => self.handle_insert_key(modifiers, code),
            VimMode::Normal => self.handle_normal_key(modifiers, code),
            VimMode::Visual { .. } => self.handle_visual_key(modifiers, code),
        }
    }
}

impl CodeInputFieldState {
    /// Plain single-mode editing: chars insert, Enter splits with
    /// auto-indent, Backspace/Delete edit, arrows navigate with wrap. Used
    /// directly by the legacy (`vim == false`) path, and by vim's Insert mode
    /// for every key it doesn't intercept itself (Esc/Tab/BackTab).
    fn handle_edit_key(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        // The space-chord path manages `last_space` itself; every other
        // handled key clears it so a stray keypress between two spaces
        // cancels the pending double-space-indent expansion.
        if !matches!(
            (modifiers, code),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(' '))
        ) {
            self.last_space = None;
        }
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.active_line > 0 {
                    self.active_line -= 1;
                    self.clamp_cursor();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                    self.clamp_cursor();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.active_line > 0 {
                    self.active_line -= 1;
                    self.cursor_col = self.lines[self.active_line].chars().count();
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                let line_len = self.lines[self.active_line].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.active_line + 1 < self.lines.len() {
                    self.active_line += 1;
                    self.cursor_col = 0;
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Enter) if !self.disabled => {
                let line = self.lines[self.active_line].clone();
                let chars: Vec<char> = line.chars().collect();
                let before: String = chars[..self.cursor_col].iter().collect();
                let after: String = chars[self.cursor_col..].iter().collect();
                // Auto-indent: inherit the split line's leading whitespace, one level
                // deeper/shallower per its net block balance (format-on-blur trues it up).
                let indent = match self.language {
                    Some(lang) => {
                        let lead = before.chars().take_while(|c| *c == ' ').count() as i32;
                        let delta = ferrowl_syntax::indent_delta(lang, &before);
                        (lead + 4 * delta).max(0) as usize
                    }
                    None => 0,
                };
                self.lines[self.active_line] = before;
                self.active_line += 1;
                self.lines
                    .insert(self.active_line, format!("{}{}", " ".repeat(indent), after));
                self.cursor_col = indent;
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Backspace) if !self.disabled => {
                if self.cursor_col > 0 {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let new_line: String = chars
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != self.cursor_col - 1)
                        .map(|(_, c)| *c)
                        .collect();
                    self.lines[self.active_line] = new_line;
                    self.cursor_col -= 1;
                } else if self.active_line > 0 {
                    let current = self.lines.remove(self.active_line);
                    self.active_line -= 1;
                    self.cursor_col = self.lines[self.active_line].chars().count();
                    self.lines[self.active_line].push_str(&current);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Delete) if !self.disabled => {
                let line_len = self.lines[self.active_line].chars().count();
                if self.cursor_col < line_len {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let new_line: String = chars
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != self.cursor_col)
                        .map(|(_, c)| *c)
                        .collect();
                    self.lines[self.active_line] = new_line;
                } else if self.active_line + 1 < self.lines.len() {
                    let next = self.lines.remove(self.active_line + 1);
                    self.lines[self.active_line].push_str(&next);
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(' ')) if !self.disabled => {
                if let (Some(threshold), Some((at, line, col))) =
                    (self.space_indent, self.last_space)
                    && line == self.active_line
                    && col == self.cursor_col
                    && at.elapsed() <= threshold
                {
                    let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                    let mut new_chars = Vec::with_capacity(chars.len() + 3);
                    new_chars.extend_from_slice(&chars[..self.cursor_col]);
                    new_chars.extend([' ', ' ', ' ']);
                    new_chars.extend_from_slice(&chars[self.cursor_col..]);
                    self.lines[self.active_line] = new_chars.into_iter().collect();
                    self.cursor_col += 3;
                    self.last_space = None;
                    return EventResult::Consumed;
                }
                let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + 1);
                new_chars.extend_from_slice(&chars[..self.cursor_col]);
                new_chars.push(' ');
                new_chars.extend_from_slice(&chars[self.cursor_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col += 1;
                if self.space_indent.is_some() {
                    self.last_space = Some((Instant::now(), self.active_line, self.cursor_col));
                }
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) if !self.disabled => {
                let chars: Vec<char> = self.lines[self.active_line].chars().collect();
                let mut new_chars = Vec::with_capacity(chars.len() + 1);
                new_chars.extend_from_slice(&chars[..self.cursor_col]);
                new_chars.push(c);
                new_chars.extend_from_slice(&chars[self.cursor_col..]);
                self.lines[self.active_line] = new_chars.into_iter().collect();
                self.cursor_col += 1;
                EventResult::Consumed
            }
            (m, c) => EventResult::Unhandled(m, c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CodeInputFieldState {
        CodeInputFieldStateBuilder::default()
            .vim(false)
            .build()
            .unwrap()
    }

    fn press(s: &mut CodeInputFieldState, modifiers: KeyModifiers, code: KeyCode) {
        s.handle_events(modifiers, code);
    }

    fn type_char(s: &mut CodeInputFieldState, c: char) {
        press(s, KeyModifiers::NONE, KeyCode::Char(c));
    }

    fn backspace(s: &mut CodeInputFieldState) {
        press(s, KeyModifiers::NONE, KeyCode::Backspace);
    }

    #[test]
    fn type_and_delete() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        assert_eq!(s.content(), "abc");
        assert_eq!(s.cursor_col(), 3);
        backspace(&mut s);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.cursor_col(), 2);
        backspace(&mut s);
        assert_eq!(s.content(), "a");
        assert_eq!(s.cursor_col(), 1);
        backspace(&mut s);
        assert_eq!(s.content(), "");
        assert_eq!(s.cursor_col(), 0);
        backspace(&mut s);
        assert_eq!(s.content(), "");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn backspace_mid_line() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        backspace(&mut s);
        assert_eq!(s.content(), "ac");
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn backspace_merges_lines() {
        let mut s = state();
        type_char(&mut s, 'a');
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        type_char(&mut s, 'b');
        assert_eq!(s.content(), "a\nb");
        assert_eq!(s.active_line(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        backspace(&mut s);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn delete_forward() {
        let mut s = state();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        type_char(&mut s, 'c');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "ac");
        assert_eq!(s.cursor_col(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "a");
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "a");
    }

    #[test]
    fn delete_merges_next_line() {
        let mut s = state();
        type_char(&mut s, 'a');
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        type_char(&mut s, 'b');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Delete);
        assert_eq!(s.content(), "ab");
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn set_content_cursor_at_end() {
        let mut s = state();
        s.set_content("hello\nworld");
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 5);
        backspace(&mut s);
        assert_eq!(s.content(), "hello\nworl");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn up_down_navigate_and_clamp_to_shorter_line() {
        let mut s = state();
        s.set_content("longline\nx");
        // Cursor sits at end of "x" (line 1, col 1). Move up onto "longline".
        press(&mut s, KeyModifiers::NONE, KeyCode::Up);
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 1);
        // Move back down; cursor clamps to the shorter line's length.
        press(&mut s, KeyModifiers::NONE, KeyCode::Down);
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn up_at_first_line_and_down_at_last_are_noops() {
        let mut s = state();
        s.set_content("a\nb");
        press(&mut s, KeyModifiers::NONE, KeyCode::Down); // already last line
        assert_eq!(s.active_line(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Up);
        assert_eq!(s.active_line(), 0);
        press(&mut s, KeyModifiers::NONE, KeyCode::Up); // already first line
        assert_eq!(s.active_line(), 0);
    }

    #[test]
    fn left_wraps_to_previous_line_end() {
        let mut s = state();
        s.set_content("ab\ncd");
        // At line 1, col 0 (move there from end).
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // col 2 -> 1
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // col 1 -> 0
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // wrap to prev line end
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn right_wraps_to_next_line_start() {
        let mut s = state();
        s.set_content("ab\ncd");
        press(&mut s, KeyModifiers::NONE, KeyCode::Up); // line 0, clamp col to 2
        // cursor is at col 2 (end of "ab"); Right wraps to next line start.
        press(&mut s, KeyModifiers::NONE, KeyCode::Right);
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn enter_splits_line_at_cursor() {
        let mut s = state();
        s.set_content("abcd");
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Left); // cursor at col 2
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "ab\ncd");
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn multibyte_chars_count_by_character() {
        let mut s = state();
        type_char(&mut s, 'é');
        type_char(&mut s, '语');
        type_char(&mut s, 'x');
        assert_eq!(s.content(), "é语x");
        assert_eq!(s.cursor_col(), 3);
        backspace(&mut s);
        assert_eq!(s.content(), "é语");
        assert_eq!(s.cursor_col(), 2);
        // Insert between the two multi-byte chars.
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        type_char(&mut s, 'Z');
        assert_eq!(s.content(), "éZ语");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn disabled_state_ignores_keys() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .disabled(true)
            .build()
            .unwrap();
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Char('a'));
        assert!(matches!(r, EventResult::Unhandled(..)));
        assert_eq!(s.content(), "");
    }

    #[test]
    fn double_space_expands_to_four_within_threshold() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn intervening_key_between_spaces_cancels_expansion() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        press(&mut s, KeyModifiers::NONE, KeyCode::Left);
        press(&mut s, KeyModifiers::NONE, KeyCode::Right);
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "  ");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn space_indent_disabled_inserts_plain_spaces() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .space_indent(None)
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "  ");
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn double_space_expansion_mid_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        s.set_content("ab");
        s.set_cursor_col(1);
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "a    b");
        assert_eq!(s.cursor_col(), 5);
    }

    #[test]
    fn four_rapid_space_presses_yield_eight_spaces() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .space_indent(Some(Duration::from_secs(600)))
            .build()
            .unwrap();
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        type_char(&mut s, ' ');
        assert_eq!(s.content(), "        ");
        assert_eq!(s.cursor_col(), 8);
    }

    #[test]
    fn blur_formats_messy_json() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":[1,2]}"#);
        s.set_focused(false);
        assert_eq!(
            s.content(),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
    }

    #[test]
    fn blur_leaves_invalid_json_unchanged() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        s.set_content("{\"a\": ");
        s.set_focused(false);
        assert_eq!(s.content(), "{\"a\": ");
    }

    #[test]
    fn disabled_field_never_formats_on_blur() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Json))
            .disabled(true)
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":2}"#);
        s.set_focused(false);
        assert_eq!(s.content(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn gaining_focus_never_formats() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Json))
            .focused(false)
            .build()
            .unwrap();
        s.set_content(r#"{"b":1,"a":2}"#);
        s.set_focused(true);
        assert_eq!(s.content(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn enter_auto_indents_after_lua_opener() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "function foo()\n    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_inherits_indent_on_plain_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()\n    print(1)");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "function foo()\n    print(1)\n    ");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_does_not_indent_after_closing_line() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("end");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "end\n");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn enter_auto_indents_json_and_carries_tail() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Json))
            .build()
            .unwrap();
        // Cursor between `{` and `}`: the tail moves to the new, indented line.
        s.set_content("{}");
        s.set_cursor_col(1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "{\n    }");
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn enter_without_language_does_not_indent() {
        let mut s = state();
        s.set_content("    x {");
        press(&mut s, KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.content(), "    x {\n");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn blur_reindents_lua() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()\nprint(1)\nend");
        s.set_focused(false);
        assert_eq!(s.content(), "function foo()\n    print(1)\nend");
    }

    // -- vim mode -------------------------------------------------------

    fn vim_state() -> CodeInputFieldState {
        CodeInputFieldStateBuilder::default().build().unwrap()
    }

    fn key(s: &mut CodeInputFieldState, c: char) -> EventResult {
        s.handle_events(KeyModifiers::NONE, KeyCode::Char(c))
    }

    #[test]
    fn vim_defaults_to_normal_mode() {
        let s = vim_state();
        assert_eq!(s.vim_mode(), VimMode::Normal);
        assert_eq!(s.mode_label(), Some("NORMAL"));
    }

    #[test]
    fn h_l_do_not_wrap_and_clamp_to_last_char() {
        let mut s = vim_state();
        s.set_content("abc");
        assert_eq!(s.cursor_col(), 2); // Normal clamp: len-1
        key(&mut s, 'l');
        assert_eq!(s.cursor_col(), 2); // already at last char, no wrap/overflow
        key(&mut s, 'h');
        key(&mut s, 'h');
        assert_eq!(s.cursor_col(), 0);
        key(&mut s, 'h');
        assert_eq!(s.cursor_col(), 0); // no wrap to previous line
    }

    #[test]
    fn j_k_clamp_to_shorter_line() {
        let mut s = vim_state();
        s.set_content("longline\nx");
        key(&mut s, 'k');
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 0); // clamped from wherever "x" ended up
        key(&mut s, 'j');
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn zero_and_dollar_motions() {
        let mut s = vim_state();
        s.set_content("hello");
        key(&mut s, '0');
        assert_eq!(s.cursor_col(), 0);
        key(&mut s, '$');
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn w_b_e_handle_punctuation_and_line_crossing() {
        let mut s = vim_state();
        s.set_content("foo.bar\nbaz");
        s.set_active_line(0);
        s.set_cursor_col(0);
        key(&mut s, 'w'); // foo -> .
        assert_eq!((s.active_line(), s.cursor_col()), (0, 3));
        key(&mut s, 'w'); // . -> bar
        assert_eq!((s.active_line(), s.cursor_col()), (0, 4));
        key(&mut s, 'w'); // bar -> crosses to baz
        assert_eq!((s.active_line(), s.cursor_col()), (1, 0));
        key(&mut s, 'b'); // back to bar
        assert_eq!((s.active_line(), s.cursor_col()), (0, 4));
        s.set_active_line(0);
        s.set_cursor_col(0);
        key(&mut s, 'e'); // end of foo
        assert_eq!((s.active_line(), s.cursor_col()), (0, 2));
    }

    #[test]
    fn gg_and_g_capital_motions() {
        let mut s = vim_state();
        s.set_content("a\nb\nc");
        assert_eq!(s.active_line(), 2);
        key(&mut s, 'G');
        assert_eq!(s.active_line(), 2);
        assert_eq!(s.cursor_col(), 0);
        s.set_active_line(1);
        key(&mut s, 'g');
        key(&mut s, 'g');
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn pending_g_and_d_canceled_by_other_key() {
        let mut s = vim_state();
        s.set_content("a\nb\nc");
        key(&mut s, 'g');
        key(&mut s, 'x'); // cancels pending g, does its own thing (delete char)
        assert_eq!(s.active_line(), 2); // gg never fired
        s.set_content("a\nb\nc");
        key(&mut s, 'd');
        key(&mut s, 'x');
        assert_eq!(s.content(), "a\nb\n"); // dd never fired; x deleted the char
    }

    #[test]
    fn x_deletes_char_under_cursor() {
        let mut s = vim_state();
        s.set_content("abc");
        s.set_cursor_col(1);
        key(&mut s, 'x');
        assert_eq!(s.content(), "ac");
    }

    #[test]
    fn dd_deletes_line_last_line_becomes_empty() {
        let mut s = vim_state();
        s.set_content("a\nb");
        s.set_active_line(0);
        key(&mut s, 'd');
        key(&mut s, 'd');
        assert_eq!(s.content(), "b");
        assert_eq!(s.lines().len(), 1);

        key(&mut s, 'd');
        key(&mut s, 'd');
        assert_eq!(s.content(), "");
        assert_eq!(s.lines().len(), 1);
    }

    #[test]
    fn yy_and_p_linewise_paste_below_and_above() {
        let mut s = vim_state();
        s.set_content("a\nb");
        s.set_active_line(0);
        key(&mut s, 'y');
        key(&mut s, 'y');
        key(&mut s, 'p');
        assert_eq!(s.content(), "a\na\nb");
        assert_eq!(s.active_line(), 1);
        assert_eq!(s.cursor_col(), 0);

        s.set_content("a\nb");
        s.set_active_line(0);
        key(&mut s, 'y');
        key(&mut s, 'y');
        s.set_active_line(1);
        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('P'));
        assert_eq!(s.content(), "a\na\nb");
        assert_eq!(s.active_line(), 1);
    }

    #[test]
    fn charwise_yank_and_paste_after_cursor() {
        let mut s = vim_state();
        s.set_content("abc");
        s.set_cursor_col(0);
        key(&mut s, 'x'); // register = "a", content "bc"
        assert_eq!(s.content(), "bc");
        s.set_cursor_col(0);
        key(&mut s, 'p');
        assert_eq!(s.content(), "bac");
        assert_eq!(s.cursor_col(), 1);
    }

    #[test]
    fn visual_charwise_yank_across_lines() {
        let mut s = vim_state();
        s.set_content("abc\ndef");
        s.set_active_line(0);
        s.set_cursor_col(1);
        key(&mut s, 'v');
        s.set_active_line(1);
        s.set_cursor_col(1);
        key(&mut s, 'y');
        assert_eq!(s.vim_mode(), VimMode::Normal);
        assert_eq!((s.active_line(), s.cursor_col()), (0, 1));
        s.set_cursor_col(0);
        s.set_active_line(0);
        key(&mut s, 'p');
        // Multi-line charwise paste splices at the insert point: line 0's
        // tail ("bc") moves to the end of the register's last line ("de").
        assert_eq!(s.content(), "abc\ndebc\ndef");
    }

    #[test]
    fn visual_linewise_v_and_y() {
        let mut s = vim_state();
        s.set_content("a\nb\nc");
        s.set_active_line(0);
        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('V'));
        assert_eq!(s.mode_label(), Some("V-LINE"));
        s.set_active_line(1);
        key(&mut s, 'y');
        assert_eq!(s.vim_mode(), VimMode::Normal);
        assert_eq!(s.active_line(), 0);
        key(&mut s, 'p');
        assert_eq!(s.content(), "a\na\nb\nb\nc");
    }

    #[test]
    fn visual_delete_joins_lines() {
        let mut s = vim_state();
        s.set_content("abc\ndef");
        s.set_active_line(0);
        s.set_cursor_col(1);
        key(&mut s, 'v');
        s.set_active_line(1);
        s.set_cursor_col(1);
        key(&mut s, 'd');
        assert_eq!(s.content(), "af");
        assert_eq!(s.vim_mode(), VimMode::Normal);
    }

    #[test]
    fn undo_toggles_one_edit_and_one_insert_session() {
        let mut s = vim_state();
        s.set_content("abc");
        s.set_cursor_col(1);
        key(&mut s, 'x');
        assert_eq!(s.content(), "ac");
        key(&mut s, 'u');
        assert_eq!(s.content(), "abc");

        s.set_content("abc");
        s.set_cursor_col(3);
        key(&mut s, 'A');
        type_char(&mut s, 'd');
        type_char(&mut s, 'e');
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(s.content(), "abcde");
        key(&mut s, 'u');
        assert_eq!(s.content(), "abc");
    }

    #[test]
    fn insert_entry_positions() {
        let mut s = vim_state();
        s.set_content("abc");
        s.set_cursor_col(1);
        key(&mut s, 'i');
        assert_eq!(s.vim_mode(), VimMode::Insert);
        assert_eq!(s.cursor_col(), 1);
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);

        s.set_cursor_col(1);
        key(&mut s, 'a');
        assert_eq!(s.cursor_col(), 2);
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);

        s.set_content("  abc");
        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('I'));
        assert_eq!(s.cursor_col(), 2);
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);

        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('A'));
        assert_eq!(s.cursor_col(), 5);
    }

    #[test]
    fn o_opens_line_below_with_lua_auto_indent() {
        let mut s = CodeInputFieldStateBuilder::default()
            .language(Some(ferrowl_syntax::Language::Lua))
            .build()
            .unwrap();
        s.set_content("function foo()");
        key(&mut s, 'o');
        assert_eq!(s.content(), "function foo()\n    ");
        assert_eq!(s.vim_mode(), VimMode::Insert);
        assert_eq!(s.cursor_col(), 4);
    }

    #[test]
    fn shift_o_opens_line_above_same_indent() {
        let mut s = vim_state();
        s.set_content("  abc");
        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('O'));
        assert_eq!(s.content(), "  \n  abc");
        assert_eq!(s.active_line(), 0);
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn esc_from_insert_moves_cursor_left() {
        let mut s = vim_state();
        s.set_content("abc");
        key(&mut s, 'a'); // insert after 'c' at col 3
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);
        assert_eq!(s.vim_mode(), VimMode::Normal);
        assert_eq!(s.cursor_col(), 2);
    }

    #[test]
    fn esc_in_normal_is_unhandled() {
        let mut s = vim_state();
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(matches!(r, EventResult::Unhandled(..)));
    }

    #[test]
    fn tab_inserts_four_spaces_in_insert_mode() {
        let mut s = vim_state();
        s.set_content("ab");
        s.set_cursor_col(1);
        key(&mut s, 'i');
        press(&mut s, KeyModifiers::NONE, KeyCode::Tab);
        assert_eq!(s.content(), "a    b");
        assert_eq!(s.cursor_col(), 5);
    }

    #[test]
    fn backtab_dedents_partial_indent() {
        let mut s = vim_state();
        s.set_content("  abc");
        s.set_cursor_col(2);
        key(&mut s, 'i');
        press(&mut s, KeyModifiers::NONE, KeyCode::BackTab);
        assert_eq!(s.content(), "abc");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn backtab_with_shift_modifier_dedents_in_insert_mode() {
        // Crossterm actually delivers BackTab with the SHIFT modifier set
        // (see ferrowl/src/app/overlay.rs and ferrowl/src/dialog/scripts.rs),
        // not NONE — this must dedent too, not bubble to dialog focus traversal.
        let mut s = vim_state();
        s.set_content("  abc");
        s.set_cursor_col(2);
        key(&mut s, 'i');
        let r = s.handle_events(KeyModifiers::SHIFT, KeyCode::BackTab);
        assert!(matches!(r, EventResult::Consumed));
        assert_eq!(s.content(), "abc");
        assert_eq!(s.cursor_col(), 0);
    }

    #[test]
    fn tab_and_backtab_unhandled_in_normal_mode() {
        let mut s = vim_state();
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Tab);
        assert!(matches!(r, EventResult::Unhandled(..)));
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::BackTab);
        assert!(matches!(r, EventResult::Unhandled(..)));
        let r = s.handle_events(KeyModifiers::SHIFT, KeyCode::BackTab);
        assert!(matches!(r, EventResult::Unhandled(..)));
    }

    #[test]
    fn disabled_field_allows_motion_and_visual_yank_but_not_edits() {
        let mut s = CodeInputFieldStateBuilder::default()
            .disabled(true)
            .build()
            .unwrap();
        s.set_content("abc");
        s.set_cursor_col(0);
        key(&mut s, 'l');
        assert_eq!(s.cursor_col(), 1);
        key(&mut s, 'v');
        assert_eq!(s.vim_mode(), VimMode::Visual { linewise: false });
        key(&mut s, 'y');
        assert_eq!(s.vim_mode(), VimMode::Normal);

        let before = s.content();
        key(&mut s, 'x');
        assert_eq!(s.content(), before);
        key(&mut s, 'd');
        key(&mut s, 'd');
        assert_eq!(s.content(), before);
        key(&mut s, 'p');
        assert_eq!(s.content(), before);
        key(&mut s, 'i');
        assert_eq!(s.vim_mode(), VimMode::Normal);
    }

    #[test]
    fn plain_char_in_normal_mode_is_consumed_not_inserted() {
        let mut s = vim_state();
        let r = key(&mut s, 'z');
        assert!(matches!(r, EventResult::Consumed));
        assert_eq!(s.content(), "");
    }

    #[test]
    fn mode_label_values() {
        let mut s = vim_state();
        assert_eq!(s.mode_label(), Some("NORMAL"));
        key(&mut s, 'i');
        assert_eq!(s.mode_label(), Some("INSERT"));
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);
        key(&mut s, 'v');
        assert_eq!(s.mode_label(), Some("VISUAL"));
        press(&mut s, KeyModifiers::NONE, KeyCode::Esc);
        press(&mut s, KeyModifiers::SHIFT, KeyCode::Char('V'));
        assert_eq!(s.mode_label(), Some("V-LINE"));

        s.set_focused(false);
        assert_eq!(s.mode_label(), None);

        let legacy = CodeInputFieldStateBuilder::default()
            .vim(false)
            .build()
            .unwrap();
        assert_eq!(legacy.mode_label(), None);
    }

    #[test]
    fn selection_range_normalizes_when_anchor_after_cursor() {
        let mut s = vim_state();
        s.set_content("abcdef");
        s.set_cursor_col(4);
        key(&mut s, 'v');
        s.set_cursor_col(1);
        assert_eq!(s.selection_range(), Some(((0, 1), (0, 4))));
    }

    #[test]
    fn vim_false_legacy_chars_insert_immediately() {
        let mut s = CodeInputFieldStateBuilder::default()
            .vim(false)
            .build()
            .unwrap();
        type_char(&mut s, 'a');
        type_char(&mut s, 'b');
        assert_eq!(s.content(), "ab");
        assert_eq!(s.vim_mode(), VimMode::Normal); // vim state unused, stays default
    }
}
