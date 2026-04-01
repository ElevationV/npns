use crate::ui::screen::{Color, Screen, Style, pad_to_cols};

const STYLE_BORDER: Style = Style::new().fg(Color::Red).bold();
const STYLE_FILENAME: Style = Style::new().fg(Color::Yellow).bold();
const STYLE_TOGGLE_ON: Style = Style::new().fg(Color::Cyan).bold();
const STYLE_TOGGLE_OFF: Style = Style::new().fg(Color::DarkGray);
const STYLE_OPTION: Style = Style::new().fg(Color::Gray);
const STYLE_OPTION_SEL: Style = Style::new().fg(Color::White).bold().reverse();
const STYLE_INPUT: Style = Style::new().fg(Color::White).bold();
const STYLE_HINT: Style = Style::new().fg(Color::DarkGray);

const TL: &str = "┌"; const TR: &str = "┐";
const BL: &str = "└"; const BR: &str = "┘";
const H:  &str = "─"; const V:  &str = "│";

impl Screen {
    pub fn render_conflict_dialog(
        &mut self,
        filename: &str,
        options: &[&str],
        cursor: usize,
        apply_to_all: bool,
        rename_input: Option<&str>,
    ) {
        let body_rows = if rename_input.is_some() { 1u16 } else { options.len() as u16 };
        let dialog_h = (6 + body_rows).min(self.rows.saturating_sub(2));
        let dialog_w = 54u16.min(self.cols.saturating_sub(4));
        let col = (self.cols.saturating_sub(dialog_w)) / 2 + 1;
        let row = (self.rows.saturating_sub(dialog_h)) / 2 + 1;
        let inner_width = dialog_w.saturating_sub(2) as usize;

        let blank = " ".repeat(dialog_w as usize);
        for r in row..row + dialog_h {
            self.print(col, r, &blank, dialog_w as usize);
        }

        self.draw_dialog_border(col, row, dialog_w, dialog_h, " Conflict ");

        let mut current_row = row + 1;

        self.print(col + 1, current_row, "  ", 2);
        let name_max = inner_width.saturating_sub(18);
        self.print_styled(col + 3, current_row, filename, STYLE_FILENAME, name_max);
        let name_display_len = filename.len().min(name_max);
        self.print(col + 3 + name_display_len as u16, current_row,
            " already exists", inner_width.saturating_sub(name_display_len + 2));
        current_row += 2; // filename + blank

        if let Some(input_text) = rename_input {
            let prompt = format!("  Rename to: {}_", input_text);
            self.print_styled(col + 1, current_row,
                &pad_to_cols(&prompt, inner_width), STYLE_INPUT, inner_width);
        } else {
            let (toggle_str, toggle_style) = if apply_to_all {
                ("[*] Apply to all", STYLE_TOGGLE_ON)
            } else {
                ("[ ] Apply to all", STYLE_TOGGLE_OFF)
            };
            self.print_styled(col + 1, current_row,
                &format!("  {}", toggle_str), toggle_style,
                inner_width.saturating_sub(2));
            let hint = "(a / space)";
            let hint_col = col + 1 + (inner_width.saturating_sub(hint.len())) as u16;
            self.print_styled(hint_col, current_row, hint, STYLE_HINT, hint.len());
            current_row += 2; // toggle + blank

            for (index, label) in options.iter().enumerate() {
                if current_row >= row + dialog_h - 1 { break; }
                let is_selected = index == cursor;
                let marker      = if is_selected { "> " } else { "  " };
                let style       = if is_selected { STYLE_OPTION_SEL } else { STYLE_OPTION };
                self.print_styled(col + 1, current_row,
                    &pad_to_cols(&format!("  {}{}", marker, label), inner_width),
                    style, inner_width);
                current_row += 1;
            }
        }
    }

    fn draw_dialog_border(&mut self, col: u16, row: u16, width: u16, height: u16, title: &str) {
        let inner_width = width.saturating_sub(2) as usize;
        self.apply_style(STYLE_BORDER);

        self.goto(col, row);
        self.write_raw(TL);
        let title_len = title.len().min(inner_width);
        let padding_left = (inner_width.saturating_sub(title_len)) / 2;
        let padding_right = inner_width.saturating_sub(title_len + padding_left);
        for _ in 0..padding_left  { 
            self.write_raw(H); 
        }
        self.write_raw(&title[..title_len]);
        for _ in 0..padding_right { 
            self.write_raw(H); 
        }
        self.write_raw(TR);

        for r in (row + 1)..(row + height - 1) {
            self.goto(col, r); 
            self.write_raw(V);
            
            self.goto(col + width - 1, r); 
            self.write_raw(V);
        }

        self.goto(col, row + height - 1);
        self.write_raw(BL);
        for _ in 0..inner_width { 
            self.write_raw(H); 
        }
        self.write_raw(BR);

        self.reset_style();
    }
}