use crate::{
    browser::Browser,
    clipboard::Clipboard,
    document::Document,
    editor::{Editor, Mode},
    input::{Direction, movement},
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::Path;

pub enum Screen {
    Browser(Browser),
    Editor(Box<Editor>),
}
#[derive(Clone, Copy)]
pub enum Action {
    Quit,
    Browse,
}

pub struct App {
    pub screen: Screen,
    pub message: String,
    pub confirmation: Option<Action>,
    pub help: bool,
    pub running: bool,
    clipboard: Clipboard,
}

impl App {
    pub fn new(path: Option<&Path>) -> Result<Self> {
        let screen = match path {
            Some(path) if path.is_dir() => Screen::Browser(Browser::open(path)?),
            Some(path) => Screen::Editor(Box::new(Editor::new(Document::open(path)?))),
            None => Screen::Browser(Browser::open(&std::env::current_dir()?)?),
        };
        Ok(Self {
            screen,
            message: "E: edit · F: select · F1: help".into(),
            confirmation: None,
            help: false,
            running: true,
            clipboard: Clipboard::new(),
        })
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(key),
            Event::Paste(text) if !self.help && self.confirmation.is_none() => {
                if let Screen::Editor(editor) = &mut self.screen {
                    editor.paste(&text);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, key: KeyEvent) {
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q')) {
                self.help = false;
            }
            return;
        }
        if let Some(action) = self.confirmation {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.confirmation = None;
                    self.message.clear();
                }
                KeyCode::Char('s') => {
                    if self.save() {
                        self.confirmation = None;
                        self.perform(action);
                    }
                }
                KeyCode::Char('d') => {
                    self.confirmation = None;
                    self.perform(action);
                }
                _ => {}
            }
            return;
        }
        if key.code == KeyCode::F(1) {
            self.help = true;
            return;
        }
        self.message.clear();
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            let code = match key.code {
                KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
                code => code,
            };
            match code {
                KeyCode::Char('s') => {
                    self.save();
                }
                KeyCode::Char('q') => self.request(Action::Quit),
                KeyCode::Char('c') => self.copy(false),
                KeyCode::Char('x') => self.copy(true),
                KeyCode::Char('v') => self.paste(),
                KeyCode::Char('e') => self.request(Action::Browse),
                KeyCode::Char('[' | ']' | '5') | KeyCode::Up | KeyCode::Down => {
                    if let Screen::Editor(editor) = &mut self.screen
                        && editor.mode == Mode::Edit
                        && !editor.add_cursor(matches!(code, KeyCode::Char('[') | KeyCode::Up))
                    {
                        self.message = "No line in that direction".into();
                    }
                }
                // Crossterm decodes legacy Ctrl+\ as Ctrl+4, and Ctrl+] as Ctrl+5.
                KeyCode::Char('\\' | '4') => {
                    if let Screen::Editor(editor) = &mut self.screen
                        && editor.mode == Mode::Edit
                    {
                        editor.reset_cursors();
                    }
                }
                KeyCode::Char('z' | 'y') => {
                    if let Screen::Editor(editor) = &mut self.screen {
                        let redo = code == KeyCode::Char('y')
                            || key.modifiers.contains(KeyModifiers::SHIFT)
                            || key.code == KeyCode::Char('Z');
                        let changed = if !redo { editor.undo() } else { editor.redo() };
                        if !changed {
                            self.message = "No more history".into();
                        }
                    }
                }
                KeyCode::Home => {
                    if let Screen::Editor(e) = &mut self.screen {
                        e.file_home();
                    }
                }
                KeyCode::End => {
                    if let Screen::Editor(e) = &mut self.screen {
                        e.file_end();
                    }
                }
                _ => {}
            }
            return;
        }
        match &mut self.screen {
            Screen::Browser(browser) => {
                if let Some((direction, step)) = movement(key, true) {
                    match direction {
                        Direction::Up => browser.move_selection(-(step as isize)),
                        Direction::Down => browser.move_selection(step as isize),
                        Direction::Left => {
                            if let Err(e) = browser.parent() {
                                self.message = format!("{e:#}");
                            }
                        }
                        Direction::Right => self.open_selected(),
                    }
                    return;
                }
                match key.code {
                    KeyCode::Enter => self.open_selected(),
                    KeyCode::Backspace | KeyCode::Char('q') => {
                        if let Err(e) = browser.parent() {
                            self.message = format!("{e:#}");
                        }
                    }
                    KeyCode::Home => browser.selected = 0,
                    KeyCode::End => browser.selected = browser.entries.len().saturating_sub(1),
                    KeyCode::Esc => self.request(Action::Quit),
                    _ => {}
                }
            }
            Screen::Editor(editor) => {
                if let Some((direction, step)) = movement(key, editor.mode != Mode::Edit) {
                    if step > 1 {
                        editor.sprint(direction);
                    } else {
                        editor.move_cursor(direction, 1);
                    }
                    return;
                }
                match key.code {
                    KeyCode::Home => editor.home(),
                    KeyCode::End => editor.end(),
                    KeyCode::PageUp => editor.move_cursor(Direction::Up, editor.page_height),
                    KeyCode::PageDown => editor.move_cursor(Direction::Down, editor.page_height),
                    KeyCode::Esc => editor.navigate(),
                    _ if editor.mode == Mode::Edit => match key.code {
                        KeyCode::Char(c) => editor.type_char(c),
                        KeyCode::Enter => editor.insert_text("\n"),
                        KeyCode::Tab => editor.insert_text("\t"),
                        KeyCode::Backspace => editor.remove_text(true),
                        KeyCode::Delete => editor.remove_text(false),
                        _ => {}
                    },
                    KeyCode::Char('e') => editor.enter_edit(),
                    KeyCode::Char('f') => editor.enter_selection(),
                    KeyCode::Char('q') => editor.navigate(),
                    KeyCode::Delete => editor.remove_text(false),
                    KeyCode::Backspace if editor.mode == Mode::Selection => {
                        editor.remove_text(true)
                    }
                    KeyCode::Backspace => editor.move_cursor(Direction::Left, 1),
                    _ => {}
                }
            }
        }
    }

    fn copy(&mut self, cut: bool) {
        let Screen::Editor(editor) = &mut self.screen else {
            return;
        };
        let Some(text) = editor.selected_text() else {
            self.message = "No text selected".into();
            return;
        };
        let system = self.clipboard.copy(&text);
        if cut {
            editor.cut_selection();
        }
        self.message = format!(
            "{} {}",
            if cut { "Cut to" } else { "Copied to" },
            if system {
                "system clipboard"
            } else {
                "docz clipboard (desktop clipboard unavailable)"
            }
        );
    }

    fn paste(&mut self) {
        let Screen::Editor(editor) = &mut self.screen else {
            return;
        };
        match self.clipboard.paste() {
            Ok((text, system)) => {
                editor.paste(&text);
                self.message = if text.is_empty() {
                    "Clipboard is empty".into()
                } else if system {
                    "Pasted from system clipboard".into()
                } else {
                    "Pasted from docz clipboard".into()
                };
            }
            Err(error) => self.message = error.to_string(),
        }
    }

    fn open_selected(&mut self) {
        let Screen::Browser(browser) = &self.screen else {
            return;
        };
        let Some(entry) = browser.entries.get(browser.selected) else {
            return;
        };
        let result = if entry.is_dir {
            Browser::open(&entry.path).map(Screen::Browser)
        } else {
            Document::open(&entry.path).map(|doc| Screen::Editor(Box::new(Editor::new(doc))))
        };
        match result {
            Ok(screen) => self.screen = screen,
            Err(e) => self.message = format!("{e:#}"),
        }
    }

    fn save(&mut self) -> bool {
        if let Screen::Editor(editor) = &mut self.screen {
            match editor.save() {
                Ok(()) => {
                    self.message = format!("Saved {}", editor.document.path.display());
                    return true;
                }
                Err(e) => self.message = format!("Save failed: {e:#}"),
            }
        }
        false
    }

    fn request(&mut self, action: Action) {
        if matches!(&self.screen, Screen::Editor(e) if e.document.dirty) {
            self.confirmation = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            Action::Browse => {
                let directory = match &self.screen {
                    Screen::Editor(e) => e.document.path.parent().unwrap_or(Path::new(".")),
                    Screen::Browser(b) => &b.directory,
                };
                match Browser::open(directory) {
                    Ok(browser) => self.screen = Screen::Browser(browser),
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(c: char, mods: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), mods))
    }

    #[test]
    fn wasd_moves_in_navigate_but_types_in_edit_and_quit_requires_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "abcde f\nsecond").unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        app.handle(key('D', KeyModifiers::NONE));
        let Screen::Editor(e) = &app.screen else {
            panic!()
        };
        assert_eq!(e.cursor.col, 6);
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        for c in "wasd".chars() {
            app.handle(key(c, KeyModifiers::NONE));
        }
        app.handle(key('q', KeyModifiers::CONTROL));
        assert!(app.running);
        assert!(app.confirmation.is_some());
        app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(app.confirmation.is_none());
        let Screen::Editor(e) = &app.screen else {
            panic!()
        };
        assert!(e.document.buffer.text().starts_with("wasd"));
        app.handle(key('q', KeyModifiers::CONTROL));
        app.handle(key('s', KeyModifiers::NONE));
        assert!(!app.running);
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("wasd"));
    }

    #[test]
    fn browser_opens_selected_file_and_ignores_key_release() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hi").unwrap();
        let mut app = App::new(Some(dir.path())).unwrap();
        app.handle(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));
        let Screen::Browser(b) = &app.screen else {
            panic!()
        };
        assert_eq!(b.selected, 0);
        app.handle(key('s', KeyModifiers::NONE));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(matches!(app.screen, Screen::Editor(_)));
    }

    fn app(text: &str) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, text).unwrap();
        let mut app = App::new(Some(&path)).unwrap();
        app.clipboard = Clipboard::internal();
        (dir, app)
    }
    fn editor(app: &App) -> &Editor {
        let Screen::Editor(editor) = &app.screen else {
            panic!()
        };
        editor
    }

    #[test]
    fn copy_keeps_selection_cut_clears_it_and_paste_works_in_navigate() {
        let (_dir, mut app) = app("hello world");
        app.handle(key('c', KeyModifiers::CONTROL));
        assert!(app.running);
        assert_eq!(app.message, "No text selected");
        app.handle(key('f', KeyModifiers::NONE));
        for _ in 0..5 {
            app.handle(key('d', KeyModifiers::NONE));
        }
        app.handle(key('c', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).selected_text().as_deref(), Some("hello"));
        assert_eq!(editor(&app).mode, Mode::Selection);
        app.handle(key('x', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), " world");
        assert_eq!(editor(&app).mode, Mode::Navigate);
        app.handle(key('v', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "hello world");
    }

    #[test]
    fn e_retains_selection_and_typing_or_newline_replaces_it() {
        let (_dir, mut app) = app("hello world");
        app.handle(key('f', KeyModifiers::NONE));
        for _ in 0..5 {
            app.handle(key('d', KeyModifiers::NONE));
        }
        app.handle(key('e', KeyModifiers::NONE));
        assert_eq!(editor(&app).mode, Mode::Edit);
        assert_eq!(editor(&app).selected_text().as_deref(), Some("hello"));
        for c in "wasd".chars() {
            app.handle(key(c, KeyModifiers::NONE));
        }
        assert_eq!(editor(&app).document.buffer.text(), "wasd world");
        app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        app.handle(key('z', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "hello world");
        app.handle(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        app.handle(key('f', KeyModifiers::NONE));
        app.handle(key('D', KeyModifiers::NONE));
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(editor(&app).document.buffer.text(), "\nworld");
    }

    #[test]
    fn shifted_ctrl_z_redoes_and_ctrl_y_remains_a_fallback() {
        let (_dir, mut app) = app("");
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(key('a', KeyModifiers::NONE));
        app.handle(key('z', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "");
        app.handle(key('z', KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert_eq!(editor(&app).document.buffer.text(), "a");
        app.handle(key('z', KeyModifiers::CONTROL));
        app.handle(key('Z', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "a");
        app.handle(key('z', KeyModifiers::CONTROL));
        app.handle(key('y', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "a");
    }

    #[test]
    fn paragraph_sprint_and_file_bounds_extend_selection_but_escape_preserves_text() {
        let (_dir, mut app) = app("one\n\nsecond\nlast");
        app.handle(key('f', KeyModifiers::NONE));
        app.handle(key('S', KeyModifiers::NONE));
        assert_eq!(editor(&app).cursor.row, 2);
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::End,
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            editor(&app).selected_text().as_deref(),
            Some("one\n\nsecond\nlast")
        );
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Home,
            KeyModifiers::CONTROL,
        )));
        assert!(editor(&app).selection_range().is_none());
        app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(editor(&app).mode, Mode::Navigate);
        assert!(!editor(&app).document.dirty);
    }

    #[test]
    fn clipboard_paste_and_bracketed_paste_replace_selected_text() {
        let (_dir, mut app) = app("hello");
        app.clipboard.copy("é中");
        app.handle(key('f', KeyModifiers::NONE));
        app.handle(key('d', KeyModifiers::NONE));
        app.handle(key('v', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).document.buffer.text(), "é中ello");
        assert_eq!(editor(&app).mode, Mode::Navigate);
        app.handle(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        app.handle(key('f', KeyModifiers::NONE));
        app.handle(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(Event::Paste("paste\ntext".into()));
        assert_eq!(editor(&app).document.buffer.text(), "paste\ntext");
        assert_eq!(editor(&app).mode, Mode::Edit);
    }

    #[test]
    fn q_cancels_selection_help_and_prompts_but_types_in_edit() {
        let (_dir, mut app) = app("abc");
        app.handle(key('f', KeyModifiers::NONE));
        app.handle(key('d', KeyModifiers::NONE));
        app.handle(key('q', KeyModifiers::NONE));
        assert_eq!(editor(&app).mode, Mode::Navigate);
        assert!(editor(&app).anchor.is_none());
        app.handle(Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)));
        app.handle(key('q', KeyModifiers::NONE));
        assert!(!app.help);
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(key('q', KeyModifiers::NONE));
        assert_eq!(editor(&app).document.buffer.text(), "aqbc");
        app.handle(key('q', KeyModifiers::CONTROL));
        assert!(app.confirmation.is_some());
        app.handle(key('q', KeyModifiers::NONE));
        assert!(app.confirmation.is_none());
        assert!(app.running);
        assert_eq!(editor(&app).mode, Mode::Edit);
    }

    #[test]
    fn ctrl_cursor_keys_and_plain_literals_work_through_input_handler() {
        let (_dir, mut app) = app("ab\ncd\nef");
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(key(']', KeyModifiers::CONTROL));
        app.handle(key(']', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).cursors().count(), 3);
        app.handle(key('q', KeyModifiers::NONE));
        assert_eq!(editor(&app).document.buffer.text(), "qab\nqcd\nqef");
        for c in ['[', ']', '\\'] {
            app.handle(key(c, KeyModifiers::NONE));
        }
        assert_eq!(
            editor(&app).document.buffer.text(),
            "q[]\\ab\nq[]\\cd\nq[]\\ef"
        );
        assert_eq!(editor(&app).cursors().count(), 3);
        app.handle(key('\\', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).cursors().count(), 1);
        app.handle(key('X', KeyModifiers::NONE));
        assert_eq!(
            editor(&app).document.buffer.text(),
            "q[]\\Xab\nq[]\\cd\nq[]\\ef"
        );
        app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(editor(&app).mode, Mode::Navigate);
    }

    #[test]
    fn q_goes_to_parent_in_explorer_without_quitting() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let mut app = App::new(Some(&child)).unwrap();
        app.handle(key('q', KeyModifiers::NONE));
        let Screen::Browser(browser) = &app.screen else {
            panic!()
        };
        assert_eq!(
            browser.directory,
            std::fs::canonicalize(dir.path()).unwrap()
        );
        assert!(app.running);
    }

    #[test]
    fn plain_symbols_insert_in_edit_and_do_not_change_cursor_count() {
        let (_dir, mut app) = app("abc\ndef");
        app.handle(key('e', KeyModifiers::NONE));
        for c in ['[', ']', '\\'] {
            app.handle(key(c, KeyModifiers::NONE));
        }
        assert_eq!(editor(&app).document.buffer.text(), "[]\\abc\ndef");
        assert_eq!(editor(&app).cursors().count(), 1);
        assert_eq!(editor(&app).mode, Mode::Edit);
    }

    #[test]
    fn ctrl_brackets_legacy_encodings_and_arrow_fallbacks_are_edit_only() {
        let (_dir, mut app) = app("abc\ndef\nghi");
        app.handle(key(']', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).cursors().count(), 1);
        app.handle(key('s', KeyModifiers::NONE));
        app.handle(key('e', KeyModifiers::NONE));
        app.handle(key('[', KeyModifiers::CONTROL));
        app.handle(key('5', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).cursors().count(), 3);
        assert_eq!(editor(&app).document.buffer.text(), "abc\ndef\nghi");
        app.handle(key('4', KeyModifiers::CONTROL));
        assert_eq!(editor(&app).cursors().count(), 1);
        for code in [KeyCode::Up, KeyCode::Down] {
            app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL)));
        }
        assert_eq!(editor(&app).cursors().count(), 3);
        app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(editor(&app).mode, Mode::Navigate);
        assert_eq!(editor(&app).cursors().count(), 1);
    }
}
