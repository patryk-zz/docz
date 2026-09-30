use crate::{
    buffer::{Buffer, Cursor},
    document::Document,
    input::Direction,
};
use anyhow::Result;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Navigate,
    Edit,
    Selection,
}

#[derive(Clone, Copy, Debug)]
struct Caret {
    position: Cursor,
    goal_col: Option<usize>,
}

#[derive(Clone)]
struct Snapshot {
    buffer: Buffer,
    cursor: Cursor,
    extra_cursors: Vec<Caret>,
    goal_col: Option<usize>,
    add_col: Option<usize>,
}

struct Change {
    start: usize,
    end: usize,
    text: String,
}

pub struct Editor {
    pub document: Document,
    pub cursor: Cursor,
    pub mode: Mode,
    pub anchor: Option<Cursor>,
    pub top: usize,
    pub left: usize,
    pub page_height: usize,
    extra_cursors: Vec<Caret>,
    goal_col: Option<usize>,
    add_col: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    edit_before: Option<Snapshot>,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            cursor: Cursor::default(),
            mode: Mode::Navigate,
            anchor: None,
            top: 0,
            left: 0,
            page_height: 20,
            extra_cursors: Vec::new(),
            goal_col: None,
            add_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            edit_before: None,
        }
    }

    pub fn cursors(&self) -> impl Iterator<Item = Cursor> + '_ {
        std::iter::once(self.cursor).chain(self.extra_cursors.iter().map(|c| c.position))
    }

    fn deduplicate_cursors(&mut self) {
        self.document.buffer.clamp(&mut self.cursor);
        let mut seen = BTreeSet::from([self.cursor]);
        self.extra_cursors.retain_mut(|caret| {
            self.document.buffer.clamp(&mut caret.position);
            seen.insert(caret.position)
        });
    }

    pub fn add_cursor(&mut self, above: bool) -> bool {
        if self.mode != Mode::Edit {
            return false;
        }
        let edge = if above {
            self.cursors().map(|c| c.row).min().unwrap()
        } else {
            self.cursors().map(|c| c.row).max().unwrap()
        };
        let row = if above {
            edge.checked_sub(1)
        } else {
            (edge + 1 < self.document.buffer.lines.len()).then_some(edge + 1)
        };
        let Some(row) = row else {
            return false;
        };
        self.commit_edit();
        self.anchor = None;
        let col = *self
            .add_col
            .get_or_insert(self.goal_col.unwrap_or(self.cursor.col));
        self.extra_cursors.push(Caret {
            position: Cursor {
                row,
                col: col.min(self.document.buffer.line_len(row)),
            },
            goal_col: Some(col),
        });
        self.deduplicate_cursors();
        true
    }

    pub fn reset_cursors(&mut self) {
        self.commit_edit();
        self.extra_cursors.clear();
        self.add_col = None;
    }

    pub fn selection_range(&self) -> Option<(Cursor, Cursor)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then_some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }
    pub fn selected_text(&self) -> Option<String> {
        self.selection_range()
            .map(|(start, end)| self.document.buffer.range_text(start, end))
    }
    fn before_movement(&mut self) {
        if self.mode == Mode::Edit {
            self.anchor = None;
        }
        self.add_col = None;
    }

    pub fn move_cursor(&mut self, direction: Direction, steps: usize) {
        self.before_movement();
        move_position(
            &self.document.buffer,
            &mut self.cursor,
            &mut self.goal_col,
            direction,
            steps,
        );
        for caret in &mut self.extra_cursors {
            move_position(
                &self.document.buffer,
                &mut caret.position,
                &mut caret.goal_col,
                direction,
                steps,
            );
        }
        self.deduplicate_cursors();
    }
    pub fn sprint(&mut self, direction: Direction) {
        self.before_movement();
        let stops = match direction {
            Direction::Left | Direction::Right => self.document.buffer.word_starts(),
            Direction::Up | Direction::Down => self.document.buffer.paragraph_starts(),
        };
        let end = self.document.buffer.file_end();
        let jump = |cursor: Cursor| match direction {
            Direction::Left | Direction::Up => stops
                .iter()
                .rev()
                .copied()
                .find(|stop| *stop < cursor)
                .unwrap_or_default(),
            Direction::Right | Direction::Down => stops
                .iter()
                .copied()
                .find(|stop| *stop > cursor)
                .unwrap_or(end),
        };
        self.cursor = jump(self.cursor);
        self.goal_col = None;
        for caret in &mut self.extra_cursors {
            caret.position = jump(caret.position);
            caret.goal_col = None;
        }
        self.deduplicate_cursors();
    }
    pub fn home(&mut self) {
        self.before_movement();
        self.cursor.col = 0;
        self.goal_col = None;
        for caret in &mut self.extra_cursors {
            caret.position.col = 0;
            caret.goal_col = None;
        }
        self.deduplicate_cursors();
    }
    pub fn end(&mut self) {
        self.before_movement();
        self.cursor.col = self.document.buffer.line_len(self.cursor.row);
        self.goal_col = None;
        for caret in &mut self.extra_cursors {
            caret.position.col = self.document.buffer.line_len(caret.position.row);
            caret.goal_col = None;
        }
        self.deduplicate_cursors();
    }
    pub fn file_home(&mut self) {
        self.before_movement();
        self.cursor = Cursor::default();
        self.goal_col = None;
        self.extra_cursors.clear();
    }
    pub fn file_end(&mut self) {
        self.before_movement();
        self.cursor = self.document.buffer.file_end();
        self.goal_col = None;
        self.extra_cursors.clear();
    }

    pub fn enter_edit(&mut self) {
        self.mode = Mode::Edit;
    }
    pub fn enter_selection(&mut self) {
        if self.mode != Mode::Selection {
            self.reset_cursors();
            self.anchor = Some(self.cursor);
            self.mode = Mode::Selection;
        }
    }
    pub fn navigate(&mut self) {
        self.reset_cursors();
        self.anchor = None;
        self.mode = Mode::Navigate;
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            buffer: self.document.buffer.clone(),
            cursor: self.cursor,
            extra_cursors: self.extra_cursors.clone(),
            goal_col: self.goal_col,
            add_col: self.add_col,
        }
    }
    fn remember(&mut self, snapshot: Snapshot) {
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(snapshot);
        self.redo.clear();
    }

    /// All changes use offsets in the same original buffer, then apply from bottom to top.
    fn edit(&mut self, changes: Vec<Change>) {
        self.edit_with_targets(changes, None);
    }

    // Each target is an original byte offset and a distance back from its rebased
    // position. Auto-pairs land before the closer; skipped closers only move cursors.
    fn edit_with_targets(
        &mut self,
        mut changes: Vec<Change>,
        targets: Option<Vec<(usize, usize)>>,
    ) {
        changes.retain(|change| change.start != change.end || !change.text.is_empty());
        if changes.is_empty() && targets.is_none() {
            return;
        }
        changes.sort_by_key(|change| (change.start, change.end));
        // Adjacent/overlapping deletions are one range; never remove a character twice.
        let mut merged: Vec<Change> = Vec::new();
        for change in changes {
            if let Some(last) = merged.last_mut()
                && last.text.is_empty()
                && change.text.is_empty()
                && change.start <= last.end
            {
                last.end = last.end.max(change.end);
            } else {
                merged.push(change);
            }
        }
        let before = if merged.is_empty() {
            None
        } else if self.mode == Mode::Edit {
            if self.edit_before.is_none() {
                self.edit_before = Some(self.snapshot());
            }
            None
        } else {
            Some(self.snapshot())
        };
        let targets = targets.unwrap_or_else(|| {
            self.cursors()
                .map(|c| (self.document.buffer.offset(c), 0))
                .collect()
        });
        let mut text = self.document.buffer.text();
        for change in merged.iter().rev() {
            text.replace_range(change.start..change.end, &change.text);
        }
        self.document.buffer.replace_text(&text);
        let positions: Vec<Cursor> = targets
            .into_iter()
            .map(|(offset, back)| {
                self.document
                    .buffer
                    .cursor_at_offset(rebase_offset(offset, &merged).saturating_sub(back))
            })
            .collect();
        self.cursor = positions[0];
        for (caret, position) in self
            .extra_cursors
            .iter_mut()
            .zip(positions.into_iter().skip(1))
        {
            caret.position = position;
            caret.goal_col = None;
        }
        self.anchor = None;
        self.goal_col = None;
        self.add_col = None;
        self.deduplicate_cursors();
        if self.mode == Mode::Selection {
            self.mode = Mode::Navigate;
        }
        self.document.refresh_dirty();
        if let Some(before) = before
            && before.buffer != self.document.buffer
        {
            self.remember(before);
        }
    }

    pub fn type_char(&mut self, character: char) {
        let text = self.document.buffer.text();
        let selection = self.selection_range();
        let mut changes = Vec::new();
        let mut targets = Vec::new();
        for cursor in self.cursors() {
            let offset = self.document.buffer.offset(cursor);
            let (start, end) = selection
                .map(|(start, end)| {
                    (
                        self.document.buffer.offset(start),
                        self.document.buffer.offset(end),
                    )
                })
                .unwrap_or((offset, offset));
            let escaped_quote = character == '"' && quote_is_escaped(&text, start);
            if selection.is_none()
                && matches!(character, '"' | ')' | ']' | '}')
                && !escaped_quote
                && text[offset..].starts_with(character)
            {
                targets.push((
                    self.document
                        .buffer
                        .offset(self.document.buffer.next_cursor(cursor)),
                    0,
                ));
                continue;
            }
            let closer = closing_pair(character).filter(|_| !escaped_quote);
            let mut insertion = character.to_string();
            if let Some(closer) = closer {
                insertion.push(closer);
            }
            changes.push(Change {
                start,
                end,
                text: insertion,
            });
            targets.push((offset, usize::from(closer.is_some())));
        }
        self.edit_with_targets(changes, Some(targets));
    }

    /// Insert literal text, including pasted delimiters.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let text = self.document.buffer.normalize_insert(text);
        let changes = if let Some((start, end)) = self.selection_range() {
            vec![Change {
                start: self.document.buffer.offset(start),
                end: self.document.buffer.offset(end),
                text,
            }]
        } else {
            self.cursors()
                .map(|cursor| {
                    let offset = self.document.buffer.offset(cursor);
                    Change {
                        start: offset,
                        end: offset,
                        text: text.clone(),
                    }
                })
                .collect()
        };
        self.edit(changes);
    }
    pub fn remove_text(&mut self, backward: bool) {
        let text = self.document.buffer.text();
        let changes = if let Some((start, end)) = self.selection_range() {
            vec![Change {
                start: self.document.buffer.offset(start),
                end: self.document.buffer.offset(end),
                text: String::new(),
            }]
        } else {
            if self.mode == Mode::Selection {
                return;
            }
            self.cursors()
                .map(|cursor| {
                    let (start, mut end) = if backward {
                        (self.document.buffer.previous_cursor(cursor), cursor)
                    } else {
                        (cursor, self.document.buffer.next_cursor(cursor))
                    };
                    if backward && start.row == cursor.row && start != cursor {
                        let start_offset = self.document.buffer.offset(start);
                        let offset = self.document.buffer.offset(cursor);
                        let opener = text[start_offset..offset].chars().next().unwrap();
                        if let Some(closer) = closing_pair(opener)
                            && offset - start_offset == 1
                            && text[offset..].starts_with(closer)
                            && !(opener == '"' && quote_is_escaped(&text, start_offset))
                        {
                            end = self.document.buffer.next_cursor(cursor);
                        }
                    }
                    Change {
                        start: self.document.buffer.offset(start),
                        end: self.document.buffer.offset(end),
                        text: String::new(),
                    }
                })
                .collect()
        };
        self.edit(changes);
    }
    pub fn cut_selection(&mut self) {
        if self.selection_range().is_some() {
            self.commit_edit();
            self.remove_text(false);
            self.navigate();
        }
    }
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.commit_edit();
        self.insert_text(text);
        self.commit_edit();
    }
    pub fn commit_edit(&mut self) {
        if let Some(before) = self.edit_before.take()
            && before.buffer != self.document.buffer
        {
            self.remember(before);
        }
    }
    fn restore(&mut self, snapshot: Snapshot) {
        self.document.buffer = snapshot.buffer;
        self.cursor = snapshot.cursor;
        self.extra_cursors = if self.mode == Mode::Edit {
            snapshot.extra_cursors
        } else {
            Vec::new()
        };
        self.anchor = None;
        if self.mode == Mode::Selection {
            self.mode = Mode::Navigate;
        }
        self.goal_col = snapshot.goal_col;
        self.add_col = snapshot.add_col;
        self.deduplicate_cursors();
        self.document.refresh_dirty();
    }
    pub fn undo(&mut self) -> bool {
        self.commit_edit();
        if let Some(before) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(before);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        self.commit_edit();
        if let Some(after) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(after);
            true
        } else {
            false
        }
    }
    pub fn save(&mut self) -> Result<()> {
        self.commit_edit();
        self.document.save()
    }
}

fn closing_pair(character: char) -> Option<char> {
    match character {
        '"' => Some('"'),
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn quote_is_escaped(text: &str, offset: usize) -> bool {
    text[..offset]
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'\\')
        .count()
        % 2
        == 1
}

fn move_position(
    buffer: &Buffer,
    cursor: &mut Cursor,
    goal: &mut Option<usize>,
    direction: Direction,
    steps: usize,
) {
    match direction {
        Direction::Up | Direction::Down => {
            let col = *goal.get_or_insert(cursor.col);
            cursor.row = if direction == Direction::Up {
                cursor.row.saturating_sub(steps)
            } else {
                cursor.row.saturating_add(steps).min(buffer.lines.len() - 1)
            };
            cursor.col = col.min(buffer.line_len(cursor.row));
        }
        Direction::Left | Direction::Right => {
            for _ in 0..steps {
                *cursor = if direction == Direction::Left {
                    buffer.previous_cursor(*cursor)
                } else {
                    buffer.next_cursor(*cursor)
                };
            }
            *goal = None;
        }
    }
}

fn rebase_offset(offset: usize, changes: &[Change]) -> usize {
    let mut delta = 0isize;
    for change in changes {
        if offset < change.start {
            break;
        }
        if offset <= change.end {
            return change.start.saturating_add_signed(delta) + change.text.len();
        }
        delta += change.text.len() as isize - (change.end - change.start) as isize;
    }
    offset.saturating_add_signed(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> (tempfile::TempDir, Editor) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, text).unwrap();
        (dir, Editor::new(Document::open(&path).unwrap()))
    }

    #[test]
    fn openers_pair_and_closers_skip_with_atomic_undo() {
        for (opener, closer) in [('"', '"'), ('(', ')'), ('[', ']'), ('{', '}')] {
            let (_dir, mut e) = editor("");
            e.enter_edit();
            e.type_char(opener);
            assert_eq!(e.document.buffer.text(), format!("{opener}{closer}"));
            assert_eq!(e.cursor.col, 1);
            e.type_char('x');
            e.type_char(closer);
            assert_eq!(e.document.buffer.text(), format!("{opener}x{closer}"));
            assert_eq!(e.cursor.col, 3);
            assert!(e.undo());
            assert_eq!(e.document.buffer.text(), "");
            assert_eq!(e.cursor.col, 0);
            assert!(e.redo());
            assert_eq!(e.document.buffer.text(), format!("{opener}x{closer}"));
            assert_eq!(e.cursor.col, 3);
        }
    }

    #[test]
    fn nested_pairs_and_existing_closers_do_not_duplicate() {
        let (_dir, mut e) = editor("");
        e.enter_edit();
        for character in "({[\"中\"]})".chars() {
            e.type_char(character);
        }
        assert_eq!(e.document.buffer.text(), "({[\"中\"]})");
        assert_eq!(e.cursor.col, 9);
        let (_dir, mut e) = editor(")]}\"");
        e.enter_edit();
        for character in ")]}\"".chars() {
            e.type_char(character);
        }
        assert_eq!(e.cursor.col, 4);
        assert!(!e.document.dirty);
        assert!(!e.undo());
        e.type_char(')');
        assert_eq!(e.document.buffer.text(), ")]}\")");
    }

    #[test]
    fn escaped_quotes_are_literal_and_even_backslashes_allow_pairing() {
        let (_dir, mut e) = editor("");
        e.enter_edit();
        e.type_char('"');
        e.type_char('\\');
        e.type_char('"');
        assert_eq!(e.document.buffer.text(), "\"\\\"\"");
        assert_eq!(e.cursor.col, 3);
        e.type_char('x');
        e.type_char('"');
        assert_eq!(e.document.buffer.text(), "\"\\\"x\"");

        let (_dir, mut e) = editor("\\\\");
        e.cursor.col = 2;
        e.enter_edit();
        e.type_char('"');
        assert_eq!(e.document.buffer.text(), "\\\\\"\"");
        assert_eq!(e.cursor.col, 3);
    }

    #[test]
    fn pairing_replaces_forward_and_backward_multiline_selections() {
        for backward in [false, true] {
            let (_dir, mut e) = editor("a\r\n中b");
            if backward {
                e.cursor = Cursor { row: 1, col: 1 };
            }
            e.enter_selection();
            e.move_cursor(
                if backward {
                    Direction::Left
                } else {
                    Direction::Right
                },
                3,
            );
            e.enter_edit();
            e.type_char('[');
            assert_eq!(e.document.buffer.text(), "[]b");
            assert_eq!(e.cursor, Cursor { row: 0, col: 1 });
            e.type_char(']');
            assert_eq!(e.cursor.col, 2);
            assert!(e.undo());
            assert_eq!(e.document.buffer.text(), "a\r\n中b");
        }
    }

    #[test]
    fn multiple_cursors_pair_rebase_and_mix_skipping_with_insertion() {
        let (_dir, mut e) = editor("中\r\ne\u{301}\r\nx");
        e.cursor = Cursor { row: 1, col: 1 };
        e.enter_edit();
        e.add_cursor(true);
        e.add_cursor(false);
        e.type_char('(');
        assert_eq!(e.document.buffer.text(), "中()\r\ne\u{301}()\r\nx()");
        assert!(e.cursors().all(|c| c.col == 2));
        e.type_char('中');
        e.type_char(')');
        assert_eq!(e.document.buffer.text(), "中(中)\r\ne\u{301}(中)\r\nx(中)");
        assert!(e.cursors().all(|c| c.col == 4));

        let (_dir, mut e) = editor("\"\r\nx\r\n\"");
        e.cursor.row = 1;
        e.enter_edit();
        e.add_cursor(true);
        e.add_cursor(false);
        e.type_char('"');
        assert_eq!(e.document.buffer.text(), "\"\r\n\"\"x\r\n\"");
        assert!(e.cursors().all(|c| c.col == 1));
        assert!(e.undo());
        assert_eq!(e.document.buffer.text(), "\"\r\nx\r\n\"");
        assert!(e.cursors().all(|c| c.col == 0));
        assert!(e.redo());
        assert!(e.cursors().all(|c| c.col == 1));

        let (_dir, mut e) = editor(")\ntext\n)");
        e.enter_edit();
        e.add_cursor(false);
        e.add_cursor(false);
        e.type_char(')');
        assert_eq!(e.document.buffer.text(), ")\n)text\n)");
        assert!(e.cursors().all(|c| c.col == 1));
    }

    #[test]
    fn paste_is_literal_and_empty_pairs_backspace_together() {
        let (_dir, mut e) = editor("");
        e.enter_edit();
        e.paste("([\"{");
        assert_eq!(e.document.buffer.text(), "([\"{");
        assert_eq!(e.cursor.col, 4);
        for opener in ['"', '(', '[', '{'] {
            e.type_char(opener);
            e.remove_text(true);
            assert_eq!(e.document.buffer.text(), "([\"{");
            assert_eq!(e.cursor.col, 4);
        }
        e.type_char('(');
        e.type_char('x');
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "([\"{()");
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "([\"{");

        let (_dir, mut e) = editor("中\r\nx");
        e.cursor.col = 1;
        e.enter_edit();
        e.add_cursor(false);
        e.type_char('[');
        e.commit_edit();
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "中\r\nx");
        assert!(e.cursors().all(|c| c.col == 1));
        assert!(e.undo());
        assert_eq!(e.document.buffer.text(), "中[]\r\nx[]");
        assert!(e.cursors().all(|c| c.col == 2));
    }

    #[test]
    fn edit_session_undo_redo_and_save_state() {
        let (_dir, mut e) = editor("");
        e.enter_edit();
        e.insert_text("hi");
        e.insert_text(" there");
        e.navigate();
        assert!(e.document.dirty);
        assert!(e.undo());
        assert_eq!(e.document.buffer.text(), "");
        assert!(!e.document.dirty);
        assert!(e.redo());
        assert_eq!(e.document.buffer.text(), "hi there");
        e.save().unwrap();
        assert!(!e.document.dirty);
        e.undo();
        assert!(e.document.dirty);
    }

    #[test]
    fn selection_shrinks_reverses_and_crosses_lines() {
        let (_dir, mut e) = editor("abc\r\ndef");
        e.cursor.col = 2;
        e.enter_selection();
        e.move_cursor(Direction::Right, 3);
        assert_eq!(e.selected_text().as_deref(), Some("c\r\nd"));
        e.move_cursor(Direction::Left, 3);
        assert!(e.selection_range().is_none());
        e.move_cursor(Direction::Left, 1);
        assert_eq!(e.selected_text().as_deref(), Some("b"));
        e.navigate();
        assert!(e.anchor.is_none());
        assert_eq!(e.document.buffer.text(), "abc\r\ndef");
    }

    #[test]
    fn e_keeps_selection_until_first_input_and_replacement_undo_is_atomic() {
        let (_dir, mut e) = editor("hello world");
        e.enter_selection();
        e.move_cursor(Direction::Right, 5);
        e.enter_edit();
        assert_eq!(e.selected_text().as_deref(), Some("hello"));
        assert!(!e.document.dirty);
        e.insert_text("good");
        e.insert_text("bye");
        e.navigate();
        assert_eq!(e.document.buffer.text(), "goodbye world");
        e.undo();
        assert_eq!(e.document.buffer.text(), "hello world");
        e.redo();
        assert_eq!(e.document.buffer.text(), "goodbye world");
    }

    #[test]
    fn cancel_or_move_in_edit_does_not_delete_pending_selection() {
        let (_dir, mut e) = editor("hello world");
        e.enter_selection();
        e.move_cursor(Direction::Right, 5);
        e.enter_edit();
        e.navigate();
        assert!(!e.document.dirty);
        e.file_home();
        e.enter_selection();
        e.move_cursor(Direction::Right, 5);
        e.enter_edit();
        e.move_cursor(Direction::Right, 1);
        e.insert_text("!");
        assert_eq!(e.document.buffer.text(), "hello !world");
    }

    #[test]
    fn deleting_selection_does_not_delete_an_extra_character() {
        let (_dir, mut e) = editor("abcdef");
        e.enter_selection();
        e.move_cursor(Direction::Right, 3);
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "def");
        assert_eq!(e.mode, Mode::Navigate);
        e.undo();
        e.file_home();
        e.enter_selection();
        e.move_cursor(Direction::Right, 3);
        e.enter_edit();
        e.remove_text(false);
        e.navigate();
        assert_eq!(e.document.buffer.text(), "def");
    }

    #[test]
    fn cut_and_multiline_paste_are_separate_undo_steps() {
        let (_dir, mut e) = editor("ab\r\ncd\nef");
        e.cursor.col = 1;
        e.enter_selection();
        e.cursor = Cursor { row: 1, col: 1 };
        let text = e.selected_text().unwrap();
        assert_eq!(text, "b\r\nc");
        e.cut_selection();
        assert_eq!(e.document.buffer.text(), "ad\nef");
        assert_eq!(e.mode, Mode::Navigate);
        e.paste(&text);
        assert_eq!(e.document.buffer.text(), "ab\r\ncd\nef");
        e.undo();
        assert_eq!(e.document.buffer.text(), "ad\nef");
        e.undo();
        assert_eq!(e.document.buffer.text(), "ab\r\ncd\nef");
    }

    #[test]
    fn paste_replaces_selection_and_can_undo_without_undoing_prior_typing() {
        let (_dir, mut e) = editor("abc");
        e.enter_edit();
        e.insert_text("X");
        e.paste("Y");
        assert_eq!(e.document.buffer.text(), "XYabc");
        e.undo();
        assert_eq!(e.document.buffer.text(), "Xabc");
        e.undo();
        assert_eq!(e.document.buffer.text(), "abc");
        e.file_home();
        e.enter_selection();
        e.move_cursor(Direction::Right, 2);
        e.paste("中\n");
        assert_eq!(e.document.buffer.text(), "中\nc");
        assert_eq!(e.mode, Mode::Navigate);
        e.undo();
        assert_eq!(e.document.buffer.text(), "abc");
    }

    #[test]
    fn vertical_movement_remembers_column_across_short_lines() {
        let (_dir, mut e) = editor("abcdef\nx\nabcdef");
        e.cursor.col = 5;
        e.move_cursor(Direction::Down, 1);
        assert_eq!(e.cursor.col, 1);
        e.move_cursor(Direction::Down, 1);
        assert_eq!(e.cursor.col, 5);
        e.move_cursor(Direction::Down, 5);
        assert_eq!(e.cursor.row, 2);
    }

    #[test]
    fn word_sprint_handles_unicode_underscores_punctuation_and_newlines() {
        let (_dir, mut e) = editor("one_two(é中)  next\nlast");
        let expected = [
            Cursor { row: 0, col: 7 },
            Cursor { row: 0, col: 8 },
            Cursor { row: 0, col: 10 },
            Cursor { row: 0, col: 13 },
            Cursor { row: 1, col: 0 },
        ];
        for stop in expected {
            e.sprint(Direction::Right);
            assert_eq!(e.cursor, stop);
        }
        e.sprint(Direction::Left);
        assert_eq!(e.cursor, Cursor { row: 0, col: 13 });
        e.file_home();
        e.sprint(Direction::Left);
        assert_eq!(e.cursor, Cursor::default());
        e.file_end();
        e.sprint(Direction::Right);
        assert_eq!(e.cursor, e.document.buffer.file_end());
    }

    #[test]
    fn paragraph_sprint_skips_whitespace_separators_and_extends_selection() {
        let (_dir, mut e) = editor("first\nmore\n\n \t\nsecond\nmore\n\nthird");
        e.enter_selection();
        e.sprint(Direction::Down);
        assert_eq!(e.cursor, Cursor { row: 4, col: 0 });
        assert_eq!(e.selected_text().as_deref(), Some("first\nmore\n\n \t\n"));
        e.sprint(Direction::Down);
        assert_eq!(e.cursor.row, 7);
        e.sprint(Direction::Up);
        assert_eq!(e.cursor.row, 4);
        e.cursor = Cursor { row: 5, col: 2 };
        e.sprint(Direction::Up);
        assert_eq!(e.cursor, Cursor { row: 4, col: 0 });
    }

    #[test]
    fn adds_above_and_below_at_original_column_clamps_and_stops_at_boundaries() {
        let (_dir, mut e) = editor("abcdef\nx\nabcdef\nlast");
        assert!(!e.add_cursor(false));
        e.cursor = Cursor { row: 0, col: 4 };
        e.enter_edit();
        assert!(!e.add_cursor(true));
        assert!(e.add_cursor(false));
        assert!(e.add_cursor(false));
        assert!(e.add_cursor(false));
        assert_eq!(
            e.cursors().collect::<Vec<_>>(),
            vec![
                Cursor { row: 0, col: 4 },
                Cursor { row: 1, col: 1 },
                Cursor { row: 2, col: 4 },
                Cursor { row: 3, col: 4 }
            ]
        );
        assert!(!e.add_cursor(false));
        assert!(!e.document.dirty);
        e.reset_cursors();
        assert_eq!(e.cursors().count(), 1);
        assert_eq!(e.cursor.col, 4);
        e.cursor = Cursor { row: 3, col: 4 };
        assert!(e.add_cursor(true));
        assert!(e.add_cursor(true));
        assert!(e.add_cursor(true));
        assert_eq!(e.cursors().count(), 4);
        assert!(!e.add_cursor(true));
    }

    #[test]
    fn multicursor_multiline_insert_preserves_mixed_endings_and_rebases_every_cursor() {
        let (_dir, mut e) = editor("ab\r\ncd\nEF");
        e.cursor = Cursor { row: 1, col: 1 };
        e.enter_edit();
        e.add_cursor(true);
        e.add_cursor(false);
        e.insert_text("X\nY");
        assert_eq!(e.document.buffer.text(), "aX\r\nYb\r\ncX\r\nYd\nEX\r\nYF");
        assert_eq!(e.cursor, Cursor { row: 3, col: 1 });
        assert_eq!(
            e.cursors().collect::<Vec<_>>(),
            vec![
                Cursor { row: 3, col: 1 },
                Cursor { row: 1, col: 1 },
                Cursor { row: 5, col: 1 }
            ]
        );
        e.insert_text("!");
        assert_eq!(
            e.document.buffer.text(),
            "aX\r\nY!b\r\ncX\r\nY!d\nEX\r\nY!F"
        );
        e.undo();
        assert_eq!(e.document.buffer.text(), "ab\r\ncd\nEF");
        assert_eq!(e.cursors().count(), 3);
        e.redo();
        assert_eq!(
            e.document.buffer.text(),
            "aX\r\nY!b\r\ncX\r\nY!d\nEX\r\nY!F"
        );
    }

    #[test]
    fn multicursor_backspace_removes_whole_graphemes_and_handles_line_joins() {
        let (_dir, mut e) = editor("a👩‍💻\nbe\u{301}\nc中");
        e.cursor.col = 2;
        e.enter_edit();
        e.add_cursor(false);
        e.add_cursor(false);
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "a\nb\nc");
        assert!(e.cursors().all(|c| c.col == 1));
        e.remove_text(false);
        assert_eq!(e.document.buffer.text(), "abc");
        assert_eq!(
            e.cursors().collect::<Vec<_>>(),
            vec![
                Cursor { row: 0, col: 1 },
                Cursor { row: 0, col: 2 },
                Cursor { row: 0, col: 3 }
            ]
        );
        e.end();
        assert_eq!(e.cursors().count(), 1);
        e.insert_text("!");
        assert_eq!(e.document.buffer.text(), "abc!");
    }

    #[test]
    fn overlapping_deletions_merge_cursors_and_undo_restores_them() {
        let (_dir, mut e) = editor("\n\n");
        e.enter_edit();
        e.add_cursor(false);
        e.add_cursor(false);
        e.remove_text(false);
        assert_eq!(e.document.buffer.text(), "");
        assert_eq!(e.cursors().count(), 1);
        e.undo();
        assert_eq!(e.document.buffer.text(), "\n\n");
        assert_eq!(e.cursors().count(), 3);
    }

    #[test]
    fn arrows_and_line_bounds_move_every_cursor_and_keep_vertical_goals() {
        let (_dir, mut e) = editor("abcdef\nx\nabcdef\nabcdef");
        e.cursor.col = 4;
        e.enter_edit();
        e.add_cursor(false);
        e.move_cursor(Direction::Down, 1);
        assert_eq!(
            e.cursors().collect::<Vec<_>>(),
            vec![Cursor { row: 1, col: 1 }, Cursor { row: 2, col: 4 }]
        );
        e.move_cursor(Direction::Down, 1);
        assert_eq!(
            e.cursors().collect::<Vec<_>>(),
            vec![Cursor { row: 2, col: 4 }, Cursor { row: 3, col: 4 }]
        );
        e.home();
        assert!(e.cursors().all(|c| c.col == 0));
        e.end();
        assert!(e.cursors().all(|c| c.col == 6));
        e.file_home();
        assert_eq!(e.cursors().count(), 1);
    }

    #[test]
    fn new_cursor_set_is_an_undo_boundary_and_paste_undoes_separately() {
        let (_dir, mut e) = editor("ab\ncd");
        e.enter_edit();
        e.insert_text("!");
        e.add_cursor(false);
        e.insert_text("X");
        assert_eq!(e.document.buffer.text(), "!Xab\ncXd");
        e.paste("Y");
        assert_eq!(e.document.buffer.text(), "!XYab\ncXYd");
        e.undo();
        assert_eq!(e.document.buffer.text(), "!Xab\ncXd");
        e.undo();
        assert_eq!(e.document.buffer.text(), "!ab\ncd");
        assert_eq!(e.cursors().count(), 2);
        e.undo();
        assert_eq!(e.document.buffer.text(), "ab\ncd");
        assert_eq!(e.cursors().count(), 1);
    }

    #[test]
    fn cursor_commands_clear_pending_selection_without_editing_and_cancel_resets_cursors() {
        let (_dir, mut e) = editor("abc\ndef");
        e.enter_selection();
        e.move_cursor(Direction::Right, 2);
        e.enter_edit();
        assert!(e.selection_range().is_some());
        e.add_cursor(false);
        assert!(e.anchor.is_none());
        assert!(!e.document.dirty);
        e.insert_text("X");
        assert_eq!(e.document.buffer.text(), "abXc\ndeXf");
        e.navigate();
        assert_eq!(e.cursors().count(), 1);
    }

    #[test]
    fn combining_input_keeps_all_cursors_at_grapheme_boundaries() {
        let (_dir, mut e) = editor("e\nx");
        e.cursor.col = 1;
        e.enter_edit();
        e.add_cursor(false);
        e.insert_text("\u{301}");
        assert!(e.cursors().all(|c| c.col == 1));
        e.insert_text("!");
        assert_eq!(e.document.buffer.text(), "e\u{301}!\nx\u{301}!");
        e.remove_text(true);
        e.remove_text(true);
        assert_eq!(e.document.buffer.text(), "\n");
    }
}
