use crate::ui::screen::{Color, Rect, Screen, Style, pad_to_cols, wrap_lines};

const STYLE_CONTENT: Style = Style::new().fg(Color::Gray);
const STYLE_BORDER: Style = Style::new().fg(Color::DarkGray);
const STYLE_BORDER_DIR: Style = Style::new().fg(Color::Blue);
const STYLE_DIR_ENTRY: Style = Style::new().fg(Color::Blue).bold();

const TL: &str = "┌"; const TR: &str = "┐";
const BL: &str = "└"; const BR: &str = "┘";
const H:  &str = "─"; const V:  &str = "│";

impl Screen {
    pub fn render_preview(
        &mut self,
        area:    Rect,
        title:   &str,
        content: &str,
    ) {
        if area.width < 3 || area.height < 3 { return; }
        let is_dir_listing = !content.is_empty()
            && content.lines().all(|l| l.starts_with("> ") || l.starts_with("  "));
        let border_style = if is_dir_listing { STYLE_BORDER_DIR } else { STYLE_BORDER };

        self.draw_preview_border(area, title, border_style);

        let inner_width  = area.inner_width();
        let inner_height = area.inner_height();
        let lines        = wrap_lines(content, inner_width);

        for i in 0..inner_height {
            let screen_row = area.row + 1 + i as u16;
            match lines.get(i) {
                None => {
                    // Blank padding row
                    self.print(area.col + 1, screen_row, &" ".repeat(inner_width), inner_width);
                }
                Some(line) => {
                    let style = if line.starts_with("> ") {
                        STYLE_DIR_ENTRY
                    } else {
                        STYLE_CONTENT
                    };
                    self.print_styled(area.col + 1, screen_row,
                        &pad_to_cols(line, inner_width), style, inner_width);
                }
            }
        }
    }

    fn draw_preview_border(&mut self, area: Rect, title: &str, style: Style) {
        use crate::ui::screen::{char_width, truncate_to_cols};
        if area.width < 2 || area.height < 2 { return; }
        let inner_width = area.inner_width();

        self.apply_style(style);

        self.goto(area.col, area.row);
        self.write_raw(TL);
        let label       = truncate_to_cols(title, inner_width);
        let label_width: usize = label.chars().map(char_width).sum();
        self.write_raw(label);
        for _ in 0..inner_width.saturating_sub(label_width) { self.write_raw(H); }
        self.write_raw(TR);

        for row in (area.row + 1)..(area.row + area.height - 1) {
            self.goto(area.col, row);                  self.write_raw(V);
            self.goto(area.col + area.width - 1, row); self.write_raw(V);
        }

        self.goto(area.col, area.row + area.height - 1);
        self.write_raw(BL);
        for _ in 0..inner_width { self.write_raw(H); }
        self.write_raw(BR);

        self.reset_style();
    }
}