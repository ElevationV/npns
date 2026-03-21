use std::io::{self, BufWriter, Write};
use std::os::unix::io::RawFd; // file description

// raw mode manager
pub struct RawMode {
    orig: libc::termios,   // terminal configuration
    fd: RawFd,
}

impl RawMode {
    pub fn enter() -> io::Result<Self> {
        let fd = libc::STDIN_FILENO;
        
        // get the terminal config
        let orig = unsafe {
            let mut terminal_conf = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(fd, &mut terminal_conf) != 0 {
                return Err(io::Error::last_os_error());
            }
            terminal_conf
        };

        let mut raw = orig;
        unsafe {
            // switch off all flags of cooked mode
            libc::cfmakeraw(&mut raw);
            
            // reopen OPOST: \n still maps to \r\n on output
            raw.c_oflag |= libc::OPOST;
            // block until at least 1 byte is available
            raw.c_cc[libc::VMIN]  = 1;
            // no timeopt for c_cc
            raw.c_cc[libc::VTIME] = 0;
            
            // set config for terminal 
            if libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
        }

        Ok(Self { orig, fd })
    }
}

// RAII maybe
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.fd, libc::TCSAFLUSH, &self.orig); }
    }
}

pub fn terminal_size() -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        
        
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            return (ws.ws_col, ws.ws_row);
        }
    }
    (80, 24)
}

#[allow(unused)]
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
    // Returns the SGR foreground parameter for this color.
    pub(crate) fn fg_param(self) -> &'static str {
        match self {
            Color::Black    => "30",
            Color::Red      => "31",
            Color::Green    => "32",
            Color::Yellow   => "33",
            Color::Blue     => "34",
            Color::Magenta  => "35",
            Color::Cyan     => "36",
            Color::White    => "37",
            Color::DarkGray => "90",
            Color::Gray     => "37", // standard white ?= gray on most terminals
        }
    }

    pub(crate) fn bg_param(self) -> &'static str {
        match self {
            Color::Black    => "40",
            Color::Red      => "41",
            Color::Green    => "42",
            Color::Yellow   => "43",
            Color::Blue     => "44",
            Color::Magenta  => "45",
            Color::Cyan     => "46",
            Color::White    => "47",
            Color::DarkGray => "100",
            Color::Gray     => "47",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Style {
    pub fg:      Option<Color>,
    pub bg:      Option<Color>,
    pub bold:    bool,
    pub dim:     bool,
    pub reverse: bool,
}

#[allow(unused)]
impl Style {
    pub const fn new() -> Self {
        Self { fg: None, bg: None, bold: false, dim: false, reverse: false }
    }
    pub const fn fg(mut self, c: Color) -> Self { self.fg = Some(c); self }
    pub const fn bg(mut self, c: Color) -> Self { self.bg = Some(c); self }
    pub const fn bold(mut self)         -> Self { self.bold    = true; self }
    pub const fn dim(mut self)          -> Self { self.dim     = true; self }
    pub const fn reverse(mut self)      -> Self { self.reverse = true; self }
}

// Buffered output
pub struct Screen {
    out: BufWriter<io::Stdout>,
    pub cols: u16,
    pub rows: u16,
}

#[allow(unused)]
impl Screen {
    pub fn init() -> io::Result<Self> {
        let (cols, rows) = terminal_size();
        let mut out = BufWriter::with_capacity(64 * 1024, io::stdout());
        // "\x1b[?1049h" enter alternate buffer
        // "\x1b[?25l" hide cursor
        out.write_all(b"\x1b[?1049h\x1b[?25l")?;
        out.flush()?;
        Ok(Self { out, cols, rows })
    }

    // re-query terminal size
    pub fn resize(&mut self) {
        (self.cols, self.rows) = terminal_size();
    }

    // flush the frame buffer to the terminal
    pub fn present(&mut self) {
        let _ = self.out.flush();
    }

    // exit ui
    // show cursor and leave alternate buffer
    pub fn shutdown(&mut self) {
        let _ = self.out.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = self.out.flush();
    }

    // move cursor to (col, row)
    #[inline]
    pub fn goto(&mut self, col: u16, row: u16) {
        let _ = write!(self.out, "\x1b[{};{}H", row, col);
    }

    // erase from cursor to end of the current line.
    #[inline]
    pub fn erase_line(&mut self) {
        let _ = self.out.write_all(b"\x1b[K");
    }

    // clear the entire screen
    pub fn clear_all(&mut self) {
        let _ = self.out.write_all(b"\x1b[2J\x1b[H");
    }

    // reset all SGR attributes.
    #[inline]
    pub fn reset(&mut self) {
        let _ = self.out.write_all(b"\x1b[0m");
    }

    // convert `Style` into SGR sequence and push into buffer
    // `Screen::reset()` must be called when done
    pub fn apply_style(&mut self, style: Style) {
        // build a single SGR sequence to reduce bytes written
        let mut params: heapless::Vec<&str, 8> = heapless::Vec::new();

        if style.bold    { let _ = params.push("1"); } // bold
        if style.dim     { let _ = params.push("2"); } // dim
        if style.reverse { let _ = params.push("7"); } // reverse
        if let Some(c) = style.fg { let _ = params.push(c.fg_param()); }
        if let Some(c) = style.bg { let _ = params.push(c.bg_param()); }

        if !params.is_empty() {
            let _ = write!(self.out, "\x1b[{}m", params.join(";"));
            // for example 
            // "bold reverse white-foreground" to "\x1b[1;7;37m"
        }
    }

    // print styled text at (col, row), then reset the style
    pub fn print_styled(&mut self, col: u16, row: u16, text: &str, style: Style, max_cols: u16) {
        self.goto(col, row);
        self.reset();
        self.apply_style(style);
        
        let truncated = truncate_to_cols(text, max_cols as usize);
        let _ = self.out.write_all(truncated.as_bytes());
        
        self.reset();
    }

    // print unstyled text at (col, row)
    pub fn print(&mut self, col: u16, row: u16, text: &str, max_cols: u16) {
        self.goto(col, row);
        let truncated = truncate_to_cols(text, max_cols as usize);
        let _ = self.out.write_all(truncated.as_bytes());
    }

    // write a string that is already known to fit (no truncation)
    // used internally for box-drawing characters
    #[inline]
    pub(crate) fn write_raw(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
    }
}

// truncate the string if it exceed the max column length
pub fn truncate_to_cols(string: &str, max_cols: usize) -> &str {
    if max_cols == 0 { return ""; }
    let mut cols = 0usize;
    let mut byte_idx = 0usize;
    for chr in string.chars() {
        let w = unicode_width(chr);
        if cols + w > max_cols { break; }
        cols += w;
        byte_idx += chr.len_utf8();
    }
    &string[..byte_idx]
}

// fill the rest of the col with space
pub fn pad_to_cols(s: &str, width: usize) -> String {
    let truncated = truncate_to_cols(s, width);
    let current = truncated.chars().map(unicode_width).sum::<usize>();
    let mut out = truncated.to_owned();
    for _ in current..width {
        out.push(' ');
    }
    out
}

// get display width of single character(1 for most chars, 2 for wide CJK).
fn unicode_width(c: char) -> usize {
    if matches!(c,
        '\u{1100}'..='\u{115F}'  | '\u{2E80}'..='\u{303E}'  |
        '\u{3041}'..='\u{33FF}'  | '\u{3400}'..='\u{4DBF}'  |
        '\u{4E00}'..='\u{9FFF}'  | '\u{A000}'..='\u{A4CF}'  |
        '\u{AC00}'..='\u{D7AF}'  | '\u{F900}'..='\u{FAFF}'  |
        '\u{FE10}'..='\u{FE19}'  | '\u{FE30}'..='\u{FE6F}'  |
        '\u{FF01}'..='\u{FF60}'  | '\u{FFE0}'..='\u{FFE6}'  |
        '\u{1B000}'..='\u{1B0FF}'| '\u{1F004}'..='\u{1F9FF}'
    ) { 2 } else { 1 }
}


// We use a tiny local Vec-like type so we don't pull in the `heapless` crate.
// It is only used inside `apply_style` to join SGR parameters.
mod heapless {
    pub struct Vec<T, const N: usize> {
        buf: [Option<T>; N],
        len: usize,
    }
    impl<T: Copy, const N: usize> Vec<T, N> {
        pub fn new() -> Self {
            Self { buf: [None; N], len: 0 }
        }
        pub fn push(&mut self, v: T) -> Result<(), ()> {
            if self.len < N {
                self.buf[self.len] = Some(v);
                self.len += 1;
                Ok(())
            } else {
                Err(())
            }
        }
        pub fn is_empty(&self) -> bool { self.len == 0 }
        pub fn join(&self, sep: &str) -> String
        where T: AsRef<str>,
        {
            let mut out = String::new();
            for i in 0..self.len {
                if i > 0 { out.push_str(sep); }
                if let Some(v) = &self.buf[i] { out.push_str(v.as_ref()); }
            }
            out
        }
    }
}