use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::Widget,
};
use super::file_list::{render_border, pad, truncate};

pub struct ConflictDialog<'a> {
    pub filename:     &'a str,
    pub options:      &'a [&'a str],
    pub cursor:       usize,
    pub apply_to_all: bool,
    pub rename_input: Option<&'a str>,
}

impl Widget for ConflictDialog<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let body_rows = if self.rename_input.is_some() { 1u16 } else { self.options.len() as u16 };
        let h = (6 + body_rows).min(area.height.saturating_sub(2));
        let w = 54u16.min(area.width.saturating_sub(4));
        let x = (area.width.saturating_sub(w)) / 2 + area.x;
        let y = (area.height.saturating_sub(h)) / 2 + area.y;
        let dialog = Rect { x, y, width: w, height: h };

        for row in y..y + h {
            for col in x..x + w {
                buf[(col, row)].set_char(' ').set_style(Style::new());
            }
        }

        let inner = render_border(dialog, " Conflict ", Color::Red, buf);
        for x in dialog.x..dialog.x + w {
            buf[(x, dialog.y)].set_style(Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
        }

        let inner_w = inner.width as usize;
        let mut row = inner.y;

        let name_max = inner_w.saturating_sub(16);
        let name = truncate(self.filename, name_max);
        buf.set_span(inner.x + 2, row,
            &Span::styled(name, Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            name_max as u16);
        buf.set_span(inner.x + 2 + name_max as u16, row,
            &Span::raw(" already exists"),
            inner.width.saturating_sub(name_max as u16 + 2));
        row += 2;

        if let Some(input) = self.rename_input {
            let prompt = format!("  Rename to: {}_", input);
            buf.set_span(inner.x, row,
                &Span::styled(pad(&prompt, inner_w), Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
                inner.width);
        } else {
            let (toggle, t_style) = if self.apply_to_all {
                ("[*] Apply to all", Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else {
                ("[ ] Apply to all", Style::new().fg(Color::DarkGray))
            };
            buf.set_span(inner.x + 2, row, &Span::styled(toggle, t_style), toggle.len() as u16);
            let hint = "(a / space)";
            let hint_x = inner.x + inner.width.saturating_sub(hint.len() as u16);
            buf.set_span(hint_x, row, &Span::styled(hint, Style::new().fg(Color::DarkGray)), hint.len() as u16);
            row += 2;

            for (i, label) in self.options.iter().enumerate() {
                if row >= dialog.y + h - 1 { break; }
                let selected = i == self.cursor;
                let marker = if selected { "> " } else { "  " };
                let text   = pad(&format!("  {}{}", marker, label), inner_w);
                let style  = if selected {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::new().fg(Color::Gray)
                };
                buf.set_span(inner.x, row, &Span::styled(text, style), inner.width);
                row += 1;
            }
        }
    }
}
