use std::io::{self, Read};

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
    Unknown // this should be ignored if it's not necessary
}

// read exactly one key-press from stdin
pub fn read_key() -> io::Result<KeyCode> {
    let mut buf = [0u8; 32];
    let n = io::stdin().read(&mut buf)?;
    if n == 0 {
        return Ok(KeyCode::Unknown);
    }
    Ok(parse_key(&buf[..n]))
}

fn parse_key(bytes: &[u8]) -> KeyCode {
    match bytes {
        // plain printable ASCII / UTF-8 characters
        [b] if *b >= 0x20 && *b != 0x7F => {
            // Single-byte printable
            KeyCode::Char(*b as char)
        }
        
        // multi-byte UTF-8 scalar
        bytes if bytes[0] >= 0x80 => {
            if let Ok(s) = std::str::from_utf8(bytes)
                && let Some(ch) = s.chars().next() {
                    return KeyCode::Char(ch);
                }
            KeyCode::Unknown
        }
        
        [0x0D] | [0x0A]    => KeyCode::Enter,
        [0x7F] | [0x08]    => KeyCode::Backspace,
        [0x1B]             => KeyCode::Esc,
        
        // CSI sequences: ESC [ ...
        [0x1B, 0x5B, rest @ ..] => parse_csi(rest),
        _ => KeyCode::Unknown,
    }
}

fn parse_csi(rest: &[u8]) -> KeyCode {
    match rest {
        [b'A'] => KeyCode::Up,
        [b'B'] => KeyCode::Down,
        [b'C'] => KeyCode::Right,
        [b'D'] => KeyCode::Left,
        _      => KeyCode::Unknown,
    }
}