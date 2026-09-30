use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub const SPRINT_STEP: usize = 5;

pub fn movement(key: KeyEvent, wasd: bool) -> Option<(Direction, usize)> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    let direction = match key.code {
        KeyCode::Up => Direction::Up,
        KeyCode::Down => Direction::Down,
        KeyCode::Left => Direction::Left,
        KeyCode::Right => Direction::Right,
        KeyCode::Char(c) if wasd => match c.to_ascii_lowercase() {
            'w' => Direction::Up,
            's' => Direction::Down,
            'a' => Direction::Left,
            'd' => Direction::Right,
            _ => return None,
        },
        _ => return None,
    };
    // Many terminals report shifted letters as uppercase without a SHIFT flag.
    let sprint = key.modifiers.contains(KeyModifiers::SHIFT)
        || matches!(key.code, KeyCode::Char('W' | 'A' | 'S' | 'D'));
    Some((direction, if sprint { SPRINT_STEP } else { 1 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sprint_accepts_both_terminal_encodings_without_stealing_save() {
        assert_eq!(
            movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE), true),
            Some((Direction::Up, 1))
        );
        assert_eq!(
            movement(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::NONE), true),
            Some((Direction::Up, 5))
        );
        assert_eq!(
            movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::SHIFT), true),
            Some((Direction::Up, 5))
        );
        assert_eq!(
            movement(
                KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                true
            ),
            None
        );
        assert_eq!(
            movement(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE), false),
            None
        );
    }
}
