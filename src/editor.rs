use crate::{
    buffer::{Buffer, Cursor},
    document::Document,
    input::Direction,
};
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Navigate,
    Edit,
    Selection,
}

#[derive(Clone)]
struct Snapshot {
    buffer: Buffer,
    cursor: Cursor,
}

pub struct Editor {
    pub document: Document,
    pub cursor: Cursor,
    pub mode: Mode,
    pub anchor: Option<Cursor>,
    pub top: usize,
    pub left: usize,
    pub page_height: usize,
    goal_col: Option<usize>,
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
            goal_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            edit_before: None,
        }
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
        // Edit mode's retained selection is a pending replacement, not an extending anchor.
        if self.mode == Mode::Edit {
            self.anchor = None;
        }
    }

    pub fn move_cursor(&mut self, direction: Direction, steps: usize) {
        self.before_movement();
        match direction {
            Direction::Up | Direction::Down => {
                let col = *self.goal_col.get_or_insert(self.cursor.col);
                self.cursor.row = if direction == Direction::Up {
                    self.cursor.row.saturating_sub(steps)
                } else {
                    self.cursor
                        .row
                        .saturating_add(steps)
                        .min(self.document.buffer.lines.len() - 1)
                };
                self.cursor.col = col.min(self.document.buffer.line_len(self.cursor.row));
            }
            Direction::Left | Direction::Right => {
                for _ in 0..steps {
                    if direction == Direction::Left {
                        if self.cursor.col > 0 {
                            self.cursor.col -= 1;
                        } else if self.cursor.row > 0 {
                            self.cursor.row -= 1;
                            self.cursor.col = self.document.buffer.line_len(self.cursor.row);
                        }
                    } else if self.cursor.col < self.document.buffer.line_len(self.cursor.row) {
                        self.cursor.col += 1;
                    } else if self.cursor.row + 1 < self.document.buffer.lines.len() {
                        self.cursor.row += 1;
                        self.cursor.col = 0;
                    }
                }
                self.goal_col = None;
            }
        }
    }

    pub fn sprint(&mut self, direction: Direction) {
        self.before_movement();
        let stops = match direction {
            Direction::Left | Direction::Right => self.document.buffer.word_starts(),
            Direction::Up | Direction::Down => self.document.buffer.paragraph_starts(),
        };
        self.cursor = match direction {
            Direction::Left | Direction::Up => stops
                .into_iter()
                .rev()
                .find(|stop| *stop < self.cursor)
                .unwrap_or_default(),
            Direction::Right | Direction::Down => stops
                .into_iter()
                .find(|stop| *stop > self.cursor)
                .unwrap_or_else(|| self.document.buffer.file_end()),
        };
        self.goal_col = None;
    }

    pub fn home(&mut self) {
        self.before_movement();
        self.cursor.col = 0;
        self.goal_col = None;
    }
    pub fn end(&mut self) {
        self.before_movement();
        self.cursor.col = self.document.buffer.line_len(self.cursor.row);
        self.goal_col = None;
    }
    pub fn file_home(&mut self) {
        self.before_movement();
        self.cursor = Cursor::default();
        self.goal_col = None;
    }
    pub fn file_end(&mut self) {
        self.before_movement();
        self.cursor = self.document.buffer.file_end();
        self.goal_col = None;
    }

    pub fn enter_edit(&mut self) {
        self.mode = Mode::Edit;
    }
    pub fn enter_selection(&mut self) {
        if self.mode != Mode::Selection {
            self.commit_edit();
            self.anchor = Some(self.cursor);
            self.mode = Mode::Selection;
        }
    }
    pub fn navigate(&mut self) {
        self.commit_edit();
        self.anchor = None;
        self.mode = Mode::Navigate;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            buffer: self.document.buffer.clone(),
            cursor: self.cursor,
        }
    }
    fn remember(&mut self, snapshot: Snapshot) {
        // Bounded whole-buffer history, grouped by Edit session; paste/cut are separate steps.
        if self.undo.len() == 100 {
            self.undo.remove(0);
        }
        self.undo.push(snapshot);
        self.redo.clear();
    }

    fn edit(&mut self, edit: impl FnOnce(&mut Buffer, &mut Cursor)) {
        let before = if self.mode == Mode::Edit {
            if self.edit_before.is_none() {
                self.edit_before = Some(self.snapshot());
            }
            None
        } else {
            Some(self.snapshot())
        };
        if let Some((start, end)) = self.selection_range() {
            self.cursor = self.document.buffer.delete_range(start, end);
        }
        self.anchor = None;
        edit(&mut self.document.buffer, &mut self.cursor);
        self.goal_col = None;
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

    pub fn insert_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.edit(|b, c| b.insert(c, text));
        }
    }

    pub fn remove_text(&mut self, backward: bool) {
        let selection = self.selection_range().is_some();
        if !selection && self.mode == Mode::Selection {
            return;
        }
        self.edit(|b, c| {
            if !selection {
                if backward {
                    b.backspace(c);
                } else {
                    b.delete(c);
                }
            }
        });
    }

    pub fn cut_selection(&mut self) {
        if self.selection_range().is_some() {
            self.commit_edit();
            self.edit(|_, _| {});
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
        self.anchor = None;
        if self.mode == Mode::Selection {
            self.mode = Mode::Navigate;
        }
        self.goal_col = None;
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
}
