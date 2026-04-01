//! Basic Terminal Abstract Layer
//! 
use std::io::{self, BufWriter, Write};
use std::os::unix::io::RawFd;

/// # Terminal Raw Mode
/// 
/// ## In this mode:
/// - No input buffer, characters a delivered to the program immediately as they are typed without waiting for `ENTER`
/// 
/// - No echo. If you want it, then you have to implement it manually.
/// 
/// - no special control characters. Ctrl+C, Ctrl+Z, etc. are all treated as normal characters
/// 
/// ...
/// 
/// 
pub struct RawMode {
    original: libc::termios,
    fd:       RawFd,
}

impl RawMode {
    pub fn enter() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;

        let original = unsafe {
            let mut config = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(fd, &mut config) != 0 {
                return Err(io::Error::last_os_error());
            }
            config
        };

        let mut raw = original;
        unsafe {
            libc::cfmakeraw(&mut raw);
            // keep \n → \r\n on output
            raw.c_oflag |= libc::OPOST; 
            // block until one byte arrives
            raw.c_cc[libc::VMIN]  = 1; 
             // no timeout
            raw.c_cc[libc::VTIME] = 0; 
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(Self { original, fd })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { 
            libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.original); 
        }
    }
}



pub fn terminal_size() -> (u16, u16) {
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0
            && size.ws_col > 0 && size.ws_row > 0
        {
            return (size.ws_col, size.ws_row);
        }
    }
    (80, 24)
}

/// Colors In ANSI
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    DarkGray,
    Gray,
}

impl Color {
    pub(crate) fn fg_code(self) -> &'static str {
        match self {
            Color::Black => "30",
            Color::Red => "31",
            Color::Green => "32", 
            Color::Yellow => "33",
            Color::Blue => "34", 
            Color::Magenta => "35",
            Color::Cyan => "36", 
            Color::White => "37",
            Color::DarkGray => "90", 
            Color::Gray => "37",
        }
    }

    pub(crate) fn bg_code(self) -> &'static str {
        match self {
            Color::Black => "40", 
            Color::Red => "41",
            Color::Green => "42", 
            Color::Yellow => "43",
            Color::Blue => "44", 
            Color::Magenta => "45",
            Color::Cyan => "46", 
            Color::White => "47",
            Color::DarkGray => "100", 
            Color::Gray => "47",
        }
    }
}

/// Style
/// inculde
/// foreground color, background color,
/// bold, dim, color reverse
#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub reverse: bool,
}

#[allow(dead_code)]
impl Style {
    pub const fn new() -> Self {
        Self { fg: None, bg: None, bold: false, dim: false, reverse: false }
    }
    
    pub const fn fg(mut self, color: Color) -> Self { self.fg = Some(color); self }
    pub const fn bg(mut self, color: Color) -> Self { self.bg = Some(color); self }
    pub const fn bold(mut self)             -> Self { self.bold    = true; self }
    pub const fn dim(mut self)              -> Self { self.dim     = true; self }
    pub const fn reverse(mut self)          -> Self { self.reverse = true; self }
}

/// Rect
/// Top left point coordinates and size of something
#[derive(Clone, Copy)]
pub struct Rect {
    pub col:    u16,
    pub row:    u16,
    pub width:  u16,
    pub height: u16,
}

impl Rect {
    pub fn new(col: u16, row: u16, width: u16, height: u16) -> Self {
        Self { col, row, width, height }
    }

    pub fn inner_width(self) -> usize {
        self.width.saturating_sub(2) as usize
    }

    pub fn inner_height(self) -> usize {
        self.height.saturating_sub(2) as usize
    }
}


pub struct Screen {
    out: BufWriter<io::Stdout>,
    pub cols: u16,
    pub rows: u16,
}

impl Screen {
    pub fn init() -> io::Result<Self> {
        let (cols, rows) = terminal_size();
        let mut out = BufWriter::with_capacity(64 * 1024, io::stdout());
        out.write_all(b"\x1b[?1049h\x1b[?25l")?; // alternate buffer + hide cursor
        out.flush()?;
        Ok(Self { out, cols, rows })
    }

    pub fn resize(&mut self) {
        (self.cols, self.rows) = terminal_size();
    }

    pub fn present(&mut self) {
        let _ = self.out.flush();
    }

    pub fn shutdown(&mut self) {
        let _ = self.out.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = self.out.flush();
    }

    pub fn clear_all(&mut self) {
        let _ = self.out.write_all(b"\x1b[2J\x1b[H");
    }
    
    // cursor goto
    pub fn goto(&mut self, col: u16, row: u16) {
        let _ = write!(self.out, "\x1b[{};{}H", row, col);
    }

    pub fn reset_style(&mut self) {
        let _ = self.out.write_all(b"\x1b[0m");
    }

    pub fn apply_style(&mut self, style: Style) {
        let mut params: heapless::Vec<&str, 8> = heapless::Vec::new();
        
        if style.bold { 
            let _ = params.push("1"); 
        }
        if style.dim { 
            let _ = params.push("2"); 
        }
        if style.reverse { 
            let _ = params.push("7"); 
        }
        
        if let Some(color) = style.fg { 
            let _ = params.push(color.fg_code()); 
        }
        if let Some(color) = style.bg { 
            let _ = params.push(color.bg_code()); 
        }
        if !params.is_empty() {
            let _ = write!(self.out, "\x1b[{}m", params.join(";"));
        }
    }

    pub fn write_raw(&mut self, text: &str) {
        let _ = self.out.write_all(text.as_bytes());
    }

    pub fn print(&mut self, col: u16, row: u16, text: &str, max_width: usize) {
        self.goto(col, row);
        self.write_raw(truncate_to_cols(text, max_width));
    }

    pub fn print_styled(&mut self, col: u16, row: u16, text: &str, style: Style, max_width: usize) {
        self.goto(col, row);
        self.reset_style();
        self.apply_style(style);
        self.write_raw(truncate_to_cols(text, max_width));
        self.reset_style();
    }
}

pub fn truncate_to_cols(text: &str, max_cols: usize) -> &str {
    if max_cols == 0 { return ""; }
    let mut cols = 0usize;
    let mut byte_idx = 0usize;
    for ch in text.chars() {
        let width = char_width(ch);
        if cols + width > max_cols { break; }
        cols += width;
        byte_idx += ch.len_utf8();
    }
    &text[..byte_idx]
}

pub fn pad_to_cols(text: &str, width: usize) -> String {
    let truncated = truncate_to_cols(text, width);
    let filled_cols: usize = truncated.chars().map(char_width).sum();
    let mut out = truncated.to_owned();
    for _ in filled_cols..width { out.push(' '); }
    out
}

pub fn wrap_lines(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 { return Vec::new(); }
    let mut out = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() { out.push(String::new()); continue; }
        let mut remaining = raw_line;
        while !remaining.is_empty() {
            let chunk = truncate_to_cols(remaining, max_width);
            if chunk.is_empty() {
                let skip = remaining.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                remaining = &remaining[skip..];
            } else {
                out.push(chunk.to_owned());
                remaining = &remaining[chunk.len()..];
            }
        }
    }
    out
}

pub fn char_width(ch: char) -> usize {
    if matches!(ch,
        '\u{1100}'..='\u{115F}'  | '\u{2E80}'..='\u{303E}'  |
        '\u{3041}'..='\u{33FF}'  | '\u{3400}'..='\u{4DBF}'  |
        '\u{4E00}'..='\u{9FFF}'  | '\u{A000}'..='\u{A4CF}'  |
        '\u{AC00}'..='\u{D7AF}'  | '\u{F900}'..='\u{FAFF}'  |
        '\u{FE10}'..='\u{FE19}'  | '\u{FE30}'..='\u{FE6F}'  |
        '\u{FF01}'..='\u{FF60}'  | '\u{FFE0}'..='\u{FFE6}'  |
        '\u{1B000}'..='\u{1B0FF}'| '\u{1F004}'..='\u{1F9FF}'
    ) { 2 } else { 1 }
}

/// Temporary Heapless mod
/// 
/// so we don't need to import `heapless` crate
mod heapless {
    pub struct Vec<T, const N: usize> {
        buf: [Option<T>; N],
        len: usize,
    }
    impl<T: Copy, const N: usize> Vec<T, N> {
        pub fn new() -> Self { 
            Self { buf: [None; N], len: 0 } 
        }
        
        pub fn push(&mut self, value: T) -> Result<(), ()> {
            if self.len < N {
                self.buf[self.len] = Some(value);
                self.len += 1;
                Ok(())
            } else { Err(()) }
        }
        
        pub fn is_empty(&self) -> bool { 
            self.len == 0 
        }
        
        pub fn join(&self, sep: &str) -> String where T: AsRef<str> {
            let mut out = String::new();
            for i in 0..self.len {
                if i > 0 { out.push_str(sep); }
                if let Some(v) = &self.buf[i] { out.push_str(v.as_ref()); }
            }
            out
        }
    }
}