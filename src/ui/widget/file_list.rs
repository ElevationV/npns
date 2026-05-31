use std::path::Path;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span},
    widgets::Widget,
};

const MARK_W: usize = 2;
const SIZE_W: usize = 8;
const TYPE_W: usize = 4;
const GAPS:   usize = 3;

pub struct FileList<'a> {
    pub files:         &'a [&'a Path],
    pub cursor:        usize,
    pub scroll_offset: usize,
    pub marked:        Option<usize>,
    pub title:         &'a str,
}

impl Widget for FileList<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 6 || area.height < 5 { return; }

        let inner = render_border(area, self.title, Color::Reset, buf);
        let inner_w = inner.width as usize;
        let name_w  = inner_w.saturating_sub(MARK_W + SIZE_W + TYPE_W + GAPS);

        let header_row = inner.y;
        render_columns(buf, inner.x, header_row, name_w,
            " ", "Name", "Size    ", "Type",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD));

        let sep_y = inner.y + 1;
        if sep_y < area.bottom() - 1 {
            buf[(area.x, sep_y)].set_symbol("├");
            for x in (area.x + 1)..(area.x + area.width - 1) {
                buf[(x, sep_y)].set_symbol("─");
            }
            buf[(area.x + area.width - 1, sep_y)].set_symbol("┤");
        }

        let visible_rows = (inner.height as usize).saturating_sub(2);
        for (i, path) in self.files.iter().enumerate()
            .skip(self.scroll_offset)
            .take(visible_rows)
        {
            let y = inner.y + 2 + (i - self.scroll_offset) as u16;
            if y >= area.bottom() - 1 { break; }

            let is_cursor = i == self.cursor;
            let is_marked = self.marked == Some(i);

            let mark = if is_marked { "> " } else { "  " };
            let mark_style = if is_marked {
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            buf.set_span(inner.x, y, &Span::styled(mark, mark_style), MARK_W as u16);

            let name     = file_name(path);
            let size_str = if path.is_dir() { pad(" —", SIZE_W) } else { pad(&fmt_size(path), SIZE_W) };
            let type_str = pad(file_type(path), TYPE_W);
            let name_col = inner.x + MARK_W as u16 + 1;

            if is_cursor {
                let full = format!("{} {} {}", pad(&name, name_w), size_str, type_str);
                let span = Span::styled(full, Style::new().add_modifier(Modifier::REVERSED));
                buf.set_span(name_col, y, &span, (name_w + 1 + SIZE_W + 1 + TYPE_W) as u16);
            } else {
                let name_style = if path.is_dir() {
                    Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                let dim = Style::new().fg(Color::DarkGray);
                buf.set_span(name_col,                        y, &Span::styled(pad(&name, name_w), name_style), name_w as u16);
                buf.set_span(name_col + name_w as u16 + 1,   y, &Span::styled(size_str, dim),                  SIZE_W as u16);
                buf.set_span(name_col + name_w as u16 + 2 + SIZE_W as u16, y, &Span::styled(type_str, dim),    TYPE_W as u16);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_columns(
    buf: &mut Buffer,
    x: u16, y: u16, 
    name_w: usize,
    mark: &str, 
    name: &str, 
    size: &str, 
    typ: &str, 
    style: Style)
{
    let name_col = x + MARK_W as u16 + 1;
    buf.set_span(x,                                         y, &Span::styled(pad(mark, MARK_W), style), MARK_W as u16);
    buf.set_span(name_col,                                  y, &Span::styled(pad(name, name_w), style), name_w as u16);
    buf.set_span(name_col + name_w as u16 + 1,             y, &Span::styled(pad(size, SIZE_W), style), SIZE_W as u16);
    buf.set_span(name_col + name_w as u16 + SIZE_W as u16 + 2, y, &Span::styled(pad(typ, TYPE_W), style), TYPE_W as u16);
}

fn pad(s: &str, w: usize) -> String {
    let s = truncate(s, w);
    let len: usize = s.chars().map(char_width).sum();
    let mut out = s.to_owned();
    for _ in len..w { out.push(' '); }
    out
}

fn truncate(s: &str, max: usize) -> &str {
    let mut cols = 0;
    let mut idx  = 0;
    for ch in s.chars() {
        let w = char_width(ch);
        if cols + w > max { break; }
        cols += w;
        idx  += ch.len_utf8();
    }
    &s[..idx]
}

fn char_width(ch: char) -> usize {
    if matches!(ch, '\u{1100}'..='\u{115F}' | '\u{2E80}'..='\u{303E}' |
        '\u{3041}'..='\u{33FF}' | '\u{4E00}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7AF}' |
        '\u{F900}'..='\u{FAFF}' | '\u{FF01}'..='\u{FF60}' | '\u{1F004}'..='\u{1F9FF}'
    ) { 2 } else { 1 }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn fmt_size(path: &Path) -> String {
    let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
    if bytes < 1_024            { format!("{:<5}B", bytes) }
    else if bytes < 1_024*1_024 { format!("{:<5.1}K", bytes as f64 / 1_024.0) }
    else if bytes < 1_024*1_024*1_024 { format!("{:<5.1}M", bytes as f64 / 1_048_576.0) }
    else                        { format!("{:<5.1}G", bytes as f64 / 1_073_741_824.0) }
}

fn file_type(path: &Path) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    match path.symlink_metadata() {
        Err(_) => "ERR",
        Ok(m)  => {
            let ft = m.file_type();
            if      ft.is_dir()          { "DIR"  }
            else if ft.is_symlink()      { "LINK" }
            else if ft.is_fifo()         { "FIFO" }
            else if ft.is_char_device()  { "CHAR" }
            else if ft.is_block_device() { "BLK"  }
            else if ft.is_socket()       { "SOCK" }
            else                         { "FILE" }
        }
    }
}

pub fn render_border(area: Rect, title: &str, border_color: Color, buf: &mut Buffer) -> Rect {
    let style = Style::new().fg(border_color);
    let w = area.width;
    let h = area.height;

    buf[(area.x, area.y)].set_symbol("┌").set_style(style);
    for x in (area.x + 1)..(area.x + w - 1) {
        buf[(x, area.y)].set_symbol("─").set_style(style);
    }
    buf[(area.x + w - 1, area.y)].set_symbol("┐").set_style(style);

    let title_chars: Vec<char> = title.chars().collect();
    let max_title = (w as usize).saturating_sub(2);
    for (i, ch) in title_chars.iter().take(max_title).enumerate() {
        buf[(area.x + 1 + i as u16, area.y)]
            .set_char(*ch)
            .set_style(style);
    }

    for y in (area.y + 1)..(area.y + h - 1) {
        buf[(area.x,         y)].set_symbol("│").set_style(style);
        buf[(area.x + w - 1, y)].set_symbol("│").set_style(style);
    }

    buf[(area.x, area.y + h - 1)].set_symbol("└").set_style(style);
    for x in (area.x + 1)..(area.x + w - 1) {
        buf[(x, area.y + h - 1)].set_symbol("─").set_style(style);
    }
    buf[(area.x + w - 1, area.y + h - 1)].set_symbol("┘").set_style(style);

    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width:  w.saturating_sub(2),
        height: h.saturating_sub(2),
    }
}