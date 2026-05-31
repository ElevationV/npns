use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::Widget,
};
use super::file_list::render_border;

pub struct StatusBar<'a> {
    pub status: &'a str,
    pub info:   &'a str,
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 3 { return; }
        let inner = render_border(area, "", Color::Reset, buf);
        let w = inner.width as usize;

        let padded = pad(self.status, w);
        buf.set_span(inner.x, inner.y,
            &Span::styled(padded, Style::new().fg(Color::Green)), inner.width);

        let info_w = self.info.len().min(w);
        let right_x = inner.x + (w - info_w) as u16;
        buf.set_span(right_x, inner.y,
            &Span::styled(self.info, Style::new().fg(Color::DarkGray)), info_w as u16);
    }
}

fn pad(s: &str, w: usize) -> String {
    let mut out = s.chars().take(w).collect::<String>();
    while out.len() < w { out.push(' '); }
    out
}