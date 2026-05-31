use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::Widget,
};
use super::file_list::{render_border, pad, truncate};

pub struct Preview<'a> {
    pub title:   &'a str,
    pub content: &'a str,
}

impl Widget for Preview<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 3 || area.height < 3 { return; }

        let is_dir = !self.content.is_empty()
            && self.content.lines().all(|l| l.starts_with("> ") || l.starts_with("  "));
        let border_color = if is_dir { Color::Blue } else { Color::DarkGray };

        let inner = render_border(area, self.title, border_color, buf);
        let w = inner.width as usize;

        for (i, line) in wrap(self.content, w)
            .iter()
            .enumerate()
            .take(inner.height as usize)
        {
            let y = inner.y + i as u16;
            let style = if line.starts_with("> ") {
                Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Gray)
            };
            buf.set_span(inner.x, y, &Span::styled(pad(line, w), style), inner.width);
        }
    }
}

fn wrap(text: &str, max_w: usize) -> Vec<String> {
    if max_w == 0 { return vec![]; }
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() { out.push(String::new()); continue; }
        let mut rem = line;
        while !rem.is_empty() {
            let chunk = truncate(rem, max_w);
            if chunk.is_empty() {
                rem = &rem[rem.chars().next().map(|c| c.len_utf8()).unwrap_or(1)..];
            } else {
                out.push(chunk.to_owned());
                rem = &rem[chunk.len()..];
            }
        }
    }
    out
}
