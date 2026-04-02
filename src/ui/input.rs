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
    Unknown,
}

// Blocking read — used during paste conflict dialog where we don't need
// background refresh and just want to wait for the user to decide.
pub fn read_key() -> io::Result<KeyCode> {
    let mut buf = [0u8; 32];
    let n = io::stdin().read(&mut buf)?;
    if n == 0 { return Ok(KeyCode::Unknown); }
    Ok(parse_key(&buf[..n]))
}

// Non-blocking read with a millisecond timeout.
//
// The main loop uses this instead of read_key so that even when the user
// isn't pressing anything we still return to the loop body, poll the
// preview channel, and redraw.  Without this the preview content would
// never appear until the next keypress.
//
// Uses POSIX select(2) so we don't have to set O_NONBLOCK on stdin
// (which would interfere with raw mode and other reads).
pub fn read_key_timeout(ms: u64) -> io::Result<Option<KeyCode>> {
    let mut fds: libc::fd_set = unsafe { std::mem::zeroed() };
    unsafe { libc::FD_SET(0, &mut fds) };
    let mut timeout = libc::timeval {
        tv_sec:  (ms / 1000) as libc::time_t,
        tv_usec: ((ms % 1000) * 1000) as libc::suseconds_t,
    };
    let ready = unsafe {
        libc::select(1, &mut fds, std::ptr::null_mut(), std::ptr::null_mut(), &mut timeout)
    };
    if ready <= 0 { return Ok(None); }
    let mut buf = [0u8; 32];
    let n = io::stdin().read(&mut buf)?;
    if n == 0 { return Ok(None); }
    Ok(Some(parse_key(&buf[..n])))
}

fn parse_key(bytes: &[u8]) -> KeyCode {
    match bytes {
        [b] if *b >= 0x20 && *b != 0x7F => KeyCode::Char(*b as char),
        bytes if bytes[0] >= 0x80 => {
            if let Ok(s) = std::str::from_utf8(bytes)
                && let Some(ch) = s.chars().next() {
                    return KeyCode::Char(ch);
                }
            KeyCode::Unknown
        }
        [0x0D] | [0x0A]         => KeyCode::Enter,
        [0x7F] | [0x08]         => KeyCode::Backspace,
        [0x1B]                  => KeyCode::Esc,
        [0x1B, 0x5B, rest @ ..] => parse_csi(rest),
        _                       => KeyCode::Unknown,
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