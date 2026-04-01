use crate::ui::screen::{Color, Rect, Screen, Style, pad_to_cols};

const STYLE_STATUS: Style = Style::new().fg(Color::Green);
const STYLE_INFO: Style = Style::new().fg(Color::DarkGray);

const TL: &str = "┌"; const TR: &str = "┐";
const BL: &str = "└"; const BR: &str = "┘";
const H:  &str = "─"; const V:  &str = "│";

impl Screen {
    pub fn render_status_bar(
        &mut self,
        area:   Rect,
        status: &str,
        info:   &str,
    ) {
        if area.width < 3 || area.height < 3 { return; }
        let inner_width = area.inner_width();

        self.apply_style(Style::new());
        self.goto(area.col, area.row);
        self.write_raw(TL);
        for _ in 0..inner_width { self.write_raw(H); }
        self.write_raw(TR);

        self.goto(area.col, area.row + 1);             self.write_raw(V);
        self.goto(area.col + area.width - 1, area.row + 1); self.write_raw(V);

        self.goto(area.col, area.row + 2);
        self.write_raw(BL);
        for _ in 0..inner_width { self.write_raw(H); }
        self.write_raw(BR);
        self.reset_style();

        let padded = pad_to_cols(status, inner_width);
        self.print_styled(area.col + 1, area.row + 1, &padded, STYLE_STATUS, inner_width);

        let info_width = info.len().min(inner_width);
        let right_col  = area.col + 1 + (inner_width - info_width) as u16;
        self.print_styled(right_col, area.row + 1, info, STYLE_INFO, info_width);
    }
}