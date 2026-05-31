use std::time::Duration;
use crossterm::event::{self, Event, KeyCode as CKC, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Unknown,
}

fn map(ev: KeyEvent) -> KeyCode {
    if ev.modifiers.contains(KeyModifiers::CONTROL) { return KeyCode::Unknown; }
    match ev.code {
        CKC::Char(c)   => KeyCode::Char(c),
        CKC::Enter     => KeyCode::Enter,
        CKC::Backspace => KeyCode::Backspace,
        CKC::Esc       => KeyCode::Esc,
        CKC::Up        => KeyCode::Up,
        CKC::Down      => KeyCode::Down,
        CKC::Left      => KeyCode::Left,
        CKC::Right     => KeyCode::Right,
        _              => KeyCode::Unknown,
    }
}

pub fn read_key() -> std::io::Result<KeyCode> {
    loop {
        if let Event::Key(k) = event::read()? { return Ok(map(k)); }
    }
}

pub fn read_key_timeout(ms: u64) -> std::io::Result<Option<KeyCode>> {
    if event::poll(Duration::from_millis(ms))? && let Event::Key(k) = event::read()? { 
        return Ok(Some(map(k))); 
    }
    Ok(None)
}