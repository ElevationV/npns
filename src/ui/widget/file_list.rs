use std::path::Path;
use crate::ui::screen::{Color, Rect, Screen, Style, pad_to_cols, truncate_to_cols};

const STYLE_HEADER: Style = Style::new().fg(Color::Yellow).bold();
const STYLE_DIR: Style = Style::new().fg(Color::Blue).bold();
const STYLE_MARKED: Style = Style::new().fg(Color::Cyan).bold();
const STYLE_SELECTED: Style = Style::new().reverse();
const STYLE_DIM: Style = Style::new().fg(Color::DarkGray);

const TL: &str = "┌"; const TR: &str = "┐";
const BL: &str = "└"; const BR: &str = "┘";
const H:  &str = "─"; const V:  &str = "│";
const ML: &str = "├"; const MR: &str = "┤";

const MARK_W: usize = 2;
const SIZE_W: usize = 8;
const TYPE_W: usize = 4;
const GAPS:   usize = 3;

impl Screen {
    pub fn render_file_list(
        &mut self,
        area: Rect,
        files: &[&Path],
        cursor: usize,
        scroll_offset: usize,
        marked: Option<usize>,
        title: &str,
    ) {
        if area.width < 6 || area.height < 5 { return; }

        self.draw_border(area, title);

        let inner_width = area.inner_width();
        let name_width  = inner_width.saturating_sub(MARK_W + SIZE_W + TYPE_W + GAPS);

        // Column start positions (1-based, relative to area.col + 1)
        let col_mark = area.col + 1;
        let col_name = col_mark + MARK_W as u16 + 1;
        let col_size = col_name + name_width as u16 + 1;
        let col_type = col_size + SIZE_W as u16 + 1;

        // Header row
        let header_row = area.row + 1;
        self.print_styled(col_mark, header_row, &pad_to_cols(" ", MARK_W), STYLE_HEADER, MARK_W);
        self.print_styled(col_name, header_row, &pad_to_cols("Name", name_width), STYLE_HEADER, name_width);
        self.print_styled(col_size, header_row, &pad_to_cols("Size", SIZE_W), STYLE_HEADER, SIZE_W);
        self.print_styled(col_type, header_row, &pad_to_cols("Type", TYPE_W), STYLE_HEADER, TYPE_W);

        // Separator between header and content
        self.goto(area.col, area.row + 2);
        self.apply_style(Style::new());
        self.write_raw(ML);
        for _ in 0..inner_width { self.write_raw(H); }
        self.write_raw(MR);
        self.reset_style();

        // Content rows
        let visible_rows = area.height.saturating_sub(4) as usize;

        for (index, path) in files.iter().enumerate().skip(scroll_offset).take(visible_rows) {
            let screen_row = area.row + 3 + (index - scroll_offset) as u16;
            let is_cursor = index == cursor;
            let is_marked = marked == Some(index);

            let name = file_name_str(path);
            let size_text = if path.is_dir() {
                pad_to_cols(" —", SIZE_W)
            } else {
                pad_to_cols(&format_size(path), SIZE_W)
            };
            let type_text = pad_to_cols(file_type_str(path), TYPE_W);

            // Mark cell
            let mark_text  = if is_marked { "> " } else { "  " };
            let mark_style = if is_marked { STYLE_MARKED } else { Style::new() };
            self.print_styled(col_mark, screen_row, mark_text, mark_style, MARK_W);

            if is_cursor {
                let row_text = format!(
                    "{} {} {}",
                    pad_to_cols(&name, name_width),
                    size_text,
                    type_text,
                );
                self.print_styled(col_name, screen_row, &row_text,
                    STYLE_SELECTED, name_width + 1 + SIZE_W + 1 + TYPE_W);
            } else {
                let name_style = if path.is_dir() { STYLE_DIR } else { Style::new() };
                self.print_styled(col_name, screen_row,
                    &pad_to_cols(&name, name_width), name_style, name_width);
                self.print_styled(col_size, screen_row, &size_text, STYLE_DIM, SIZE_W);
                self.print_styled(col_type, screen_row, &type_text, STYLE_DIM, TYPE_W);
            }
        }
    }

    fn draw_border(&mut self, area: Rect, title: &str) {
        if area.width < 2 || area.height < 2 { 
            return; 
        }
        let inner_width = area.inner_width();

        self.apply_style(Style::new());

        // Top edge with title
        self.goto(area.col, area.row);
        self.write_raw(TL);
        let label = truncate_to_cols(title, inner_width);
        let label_width: usize = label.chars().map(crate::ui::screen::char_width).sum();
        self.write_raw(label);
        for _ in 0..inner_width.saturating_sub(label_width) { self.write_raw(H); }
        self.write_raw(TR);

        // Side edges
        for row in (area.row + 1)..(area.row + area.height - 1) {
            self.goto(area.col, row);                 
            self.write_raw(V);
            self.goto(area.col + area.width - 1, row); 
            self.write_raw(V);
        }

        // Bottom edge
        self.goto(area.col, area.row + area.height - 1);
        self.write_raw(BL);
        for _ in 0..inner_width { self.write_raw(H); }
        self.write_raw(BR);

        self.reset_style();
    }
}

fn file_name_str(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn format_size(path: &Path) -> String {
    let bytes = match path.metadata() {
        Ok(m)  => m.len(),
        Err(_) => return "?".to_string(),
    };
    if bytes < 1_024 {
        format!("{:<5}B", bytes)
    } else if bytes < 1_024 * 1_024 {
        format!("{:<5.1}K", bytes as f64 / 1_024.0)
    } else if bytes < 1_024 * 1_024 * 1_024 {
        format!("{:<5.1}M", bytes as f64 / (1_024.0 * 1_024.0))
    } else {
        format!("{:<5.1}G", bytes as f64 / (1_024.0 * 1_024.0 * 1_024.0))
    }
}

fn file_type_str(path: &Path) -> &'static str {
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
