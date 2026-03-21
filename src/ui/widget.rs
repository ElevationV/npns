use crate::ui::screen::{Color, Screen, Style, pad_to_cols, truncate_to_cols};

struct BoxChars {
    tl: &'static str, // top-left
    tr: &'static str, // top-right
    bl: &'static str, // bottom-left
    br: &'static str, // bottom-right
    h:  &'static str, // horizontal
    v:  &'static str, // vertical
    ml: &'static str, // mid-left  (├)
    mr: &'static str, // mid-right (┤)
}

const BOX: BoxChars = BoxChars {
    tl: "┌", tr: "┐",
    bl: "└", br: "┘",
    h:  "─", v:  "│",
    ml: "├", mr: "┤",
};

#[derive(Clone, Copy)]
pub struct Rect {
    pub col: u16,
    pub row: u16,
    pub w:   u16,
    pub h:   u16,
}

impl Rect {
    pub fn new(col: u16, row: u16, w: u16, h: u16) -> Self {
        Self { col, row, w, h }
    }
}

// border
pub fn render_border(
    scr:   &mut Screen,
    area:  Rect,
    title: Option<&str>,
    style: Style,
) {
    let (col, row, w, h) = (area.col, area.row, area.w, area.h);
    if w < 2 || h < 2 { return; }

    scr.apply_style(style);

    let inner_w = (w - 2) as usize;

    // Top edge: ┌title──────┐
    // The title is written immediately after ┌ with no leading dashes,
    // matching ratatui's Block layout.  Column-count is used (not bytes)
    // so multi-byte UTF-8 titles are measured correctly.
    scr.goto(col, row);
    scr.write_raw(BOX.tl);
    if let Some(t) = title {
        let label = truncate_to_cols(t, inner_w);
        let label_cols: usize = label.chars().map(|c| {
            // reuse the same wide-char logic as truncate_to_cols
            if matches!(c,
                '\u{1100}'..='\u{115F}' | '\u{2E80}'..='\u{303E}' |
                '\u{3041}'..='\u{33FF}' | '\u{4E00}'..='\u{9FFF}' |
                '\u{AC00}'..='\u{D7AF}' | '\u{F900}'..='\u{FAFF}' |
                '\u{FF01}'..='\u{FF60}' | '\u{FFE0}'..='\u{FFE6}'
            ) { 2 } else { 1 }
        }).sum();
        scr.write_raw(label);
        let dashes = inner_w.saturating_sub(label_cols);
        for _ in 0..dashes { scr.write_raw(BOX.h); }
    } else {
        for _ in 0..inner_w { scr.write_raw(BOX.h); }
    }
    scr.write_raw(BOX.tr);

    // Side edges
    for r in (row + 1)..(row + h - 1) {
        scr.goto(col, r);
        scr.write_raw(BOX.v);
        scr.goto(col + w - 1, r);
        scr.write_raw(BOX.v);
    }

    // Bottom edge
    scr.goto(col, row + h - 1);
    scr.write_raw(BOX.bl);
    for _ in 0..inner_w { scr.write_raw(BOX.h); }
    scr.write_raw(BOX.br);

    scr.reset();
}

// table
pub struct Row {
    /// One cell string per column.
    pub cells: Vec<String>,
    /// Per-cell styles (`None` → use the row's base `style`)
    pub cell_styles: Vec<Option<Style>>,
    /// Row-level base style (applied when no per-cell override is set)
    pub style: Style,
}

impl Row {
    /// Construct a row from anything that can become a `String`.
    pub fn new(cells: Vec<impl Into<String>>) -> Self {
        let cells: Vec<String> = cells.into_iter().map(Into::into).collect();
        let n = cells.len();
        Self { cells, cell_styles: vec![None; n], style: Style::new() }
    }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
    pub fn cell_style(mut self, idx: usize, s: Style) -> Self {
        if idx < self.cell_styles.len() { self.cell_styles[idx] = Some(s); }
        self
    }
}

/// Column width specification, matching ratatui's `Constraint` subset used in
/// the original app.
#[derive(Clone, Copy)]
pub enum ColWidth {
    /// Fixed number of columns
    Fixed(u16),
    /// Fills the remaining space (at most one per table)
    Fill,
}

/// Renders a bordered, scrollable table with a header row.
///
/// * `col`, `row` — top-left corner of the outer border (1-based)
/// * `w`, `h`     — total width / height including the border
/// * `title`      — border title
/// * `header`     — header row (always shown, not scrolled)
/// * `rows`       — data rows
/// * `col_widths` — column layout (same length as the cells in each row)
/// * `selected`   — index into `rows` that is highlighted (reversed)
///
/// The inner content area starts at (col+1, row+2) because row+1 is the
/// header, and the first and last rows are border lines.
pub fn render_table(
    scr:        &mut Screen,
    area:       Rect,
    title:      &str,
    header:     &Row,
    rows:       &[Row],
    col_widths: &[ColWidth],
    selected:   Option<usize>,
) {
    let (col, row, w, h) = (area.col, area.row, area.w, area.h);
    if w < 4 || h < 4 { return; }

    render_border(scr, area, Some(title), Style::new());

    // Resolve column pixel widths
    let inner_w = (w - 2) as usize;
    let widths = resolve_widths(col_widths, inner_w);

    // Header row (row + 1, inside the border)
    let header_style = Style::new().fg(Color::Yellow).bold();
    render_row(scr, col + 1, row + 1, &widths, inner_w, header, header_style);

    // Separator between header and content (─── line)
    scr.apply_style(Style::new());
    scr.goto(col, row + 2);
    scr.write_raw(BOX.ml);
    for _ in 0..inner_w { scr.write_raw(BOX.h); }
    scr.write_raw(BOX.mr);
    scr.reset();

    // Visible rows: content area is rows [row+3 .. row+h-1)
    let visible_h = h.saturating_sub(4) as usize; // border top/bottom + header + separator
    let (scroll_offset, _) = scroll_window(rows.len(), visible_h, selected);

    for (i, data_row) in rows.iter().enumerate().skip(scroll_offset).take(visible_h) {
        let screen_row = row + 3 + (i - scroll_offset) as u16;
        let is_selected = selected == Some(i);
        let highlight = if is_selected {
            Style::new().reverse()
        } else {
            Style::new()
        };
        render_row(scr, col + 1, screen_row, &widths, inner_w, data_row, highlight);
    }
}

// Compute scroll window: returns (scroll_offset, visible_count).
pub fn scroll_window(_total: usize, visible: usize, selected: Option<usize>) -> (usize, usize) {
    let sel = selected.unwrap_or(0);
    let offset = if sel >= visible { sel + 1 - visible } else { 0 };
    (offset, visible)
}

// render one row at screen position (col, row)
fn render_row(
    scr:        &mut Screen,
    col:        u16,
    row:        u16,
    widths:     &[usize],
    inner_w:    usize,
    data:       &Row,
    base_style: Style,
) {
    let mut cur_col = col;
    for (i, (w, cell_text)) in widths.iter().zip(data.cells.iter()).enumerate() {
        let style = if base_style.reverse {
            base_style
        } else {
            data.cell_styles.get(i)
                .and_then(|s| *s)
                .unwrap_or(data.style)
        };
        let padded = pad_to_cols(cell_text, *w);
        scr.print_styled(cur_col, row, &padded, style, *w as u16);
        cur_col += *w as u16;
 
        if i + 1 < widths.len() {
            scr.print_styled(cur_col, row, " ", base_style, 1);
            cur_col += 1;
        }
    }
    if base_style.reverse {
        let used = widths.iter().sum::<usize>() + widths.len().saturating_sub(1);
        if used < inner_w {
            let tail = inner_w - used;
            let spaces = " ".repeat(tail);
            scr.print_styled(cur_col, row, &spaces, base_style, tail as u16);
        }
    }
}

// resolve column widths, distributing leftover space to `Fill` columns.
fn resolve_widths(specs: &[ColWidth], total: usize) -> Vec<usize> {
    let gaps = specs.len().saturating_sub(1); // single-space separators
    let fixed_total: usize = specs.iter().map(|s| match s {
        ColWidth::Fixed(n) => *n as usize,
        ColWidth::Fill     => 0,
    }).sum();
    let fill_count = specs.iter().filter(|s| matches!(s, ColWidth::Fill)).count();
    let remaining = total.saturating_sub(fixed_total + gaps);
    let fill_w = if fill_count > 0 { remaining / fill_count } else { 0 };

    specs.iter().map(|s| match s {
        ColWidth::Fixed(n) => *n as usize,
        ColWidth::Fill     => fill_w,
    }).collect()
}

// List
pub fn render_list(
    scr:          &mut Screen,
    area:         Rect,
    title:        &str,
    items:        &[(String, Style)],
    border_style: Style,
) {
    let (col, row, w, h) = (area.col, area.row, area.w, area.h);
    render_border(scr, area, Some(title), border_style);
    let inner_w = w.saturating_sub(2) as usize;
    let visible_h = h.saturating_sub(2) as usize;

    for (i, (text, style)) in items.iter().take(visible_h).enumerate() {
        let padded = pad_to_cols(text, inner_w);
        scr.print_styled(col + 1, row + 1 + i as u16, &padded, *style, inner_w as u16);
    }
}

// paragraph

// renders a bordered text area.
pub fn render_paragraph(
    scr:          &mut Screen,
    area:         Rect,
    title:        &str,
    text:         &str,
    text_style:   Style,
    border_style: Style,
) {
    let (col, row, w, h) = (area.col, area.row, area.w, area.h);
    render_border(scr, area, Some(title), border_style);
    let inner_w  = w.saturating_sub(2) as usize;
    let inner_h  = h.saturating_sub(2) as usize;

    // Word-wrap / line-split the content to fill the inner area
    let lines = wrap_lines(text, inner_w);
    for (i, line) in lines.iter().take(inner_h).enumerate() {
        let padded = pad_to_cols(line, inner_w);
        scr.print_styled(col + 1, row + 1 + i as u16, &padded, text_style, inner_w as u16);
    }
}

// split `text` at newlines, and wrap long lines to `max_w` columns.
fn wrap_lines(text: &str, max_w: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut remaining = raw_line;
        while !remaining.is_empty() {
            let chunk = truncate_to_cols(remaining, max_w);
            out.push(chunk.to_owned());
            remaining = &remaining[chunk.len()..];
        }
    }
    out
}

// input bar

/// renders a single-line bordered status / input bar.
pub fn render_status_bar(
    scr:   &mut Screen,
    col:   u16,
    row:   u16,
    w:     u16,
    title: &str,
    text:  &str,
    style: Style,
) {
    render_border(scr, Rect::new(col, row, w, 3), Some(title), Style::new());
    let inner_w = w.saturating_sub(2) as usize;
    let padded  = pad_to_cols(text, inner_w);
    scr.print_styled(col + 1, row + 1, &padded, style, inner_w as u16);
}

// dialog

// Clears a rectangle (for use before drawing a popup on top of other content).
pub fn clear_rect(scr: &mut Screen, area: Rect) {
    let blank = " ".repeat(area.w as usize);
    for r in area.row..area.row + area.h {
        scr.print(area.col, r, &blank, area.w);
    }
}

// A single styled line inside a dialog, analogous to ratatui's `Line`.
pub struct DialogLine {
    // (text, style) spans — printed left-to-right, no wrapping
    pub spans: Vec<(String, Style)>,
}

#[allow(dead_code)]
impl DialogLine {
    pub fn plain(s: impl Into<String>) -> Self {
        Self { spans: vec![(s.into(), Style::new())] }
    }
    pub fn styled(s: impl Into<String>, style: Style) -> Self {
        Self { spans: vec![(s.into(), style)] }
    }
    pub fn empty() -> Self {
        Self { spans: vec![] }
    }
    pub fn push(mut self, s: impl Into<String>, style: Style) -> Self {
        self.spans.push((s.into(), style));
        self
    }
}

pub fn render_dialog(
    scr:          &mut Screen,
    w:            u16,
    h:            u16,
    title:        &str,
    lines:        &[DialogLine],
    border_style: Style,
) {
    let w = w.min(scr.cols.saturating_sub(4));
    let h = h.min(scr.rows.saturating_sub(2));
    let col = (scr.cols.saturating_sub(w)) / 2 + 1;
    let row = (scr.rows.saturating_sub(h)) / 2 + 1;

    clear_rect(scr, Rect::new(col, row, w, h));
    render_border(scr, Rect::new(col, row, w, h), Some(title), border_style);

    let inner_w = w.saturating_sub(2) as usize;
    for (i, line) in lines.iter().take(h.saturating_sub(2) as usize).enumerate() {
        let screen_row = row + 1 + i as u16;
        let mut cur_col = col + 1;
        for (text, style) in &line.spans {
            let available = (col + w - 1).saturating_sub(cur_col) as usize;
            if available == 0 { break; }
            let truncated = truncate_to_cols(text, available);
            scr.print_styled(cur_col, screen_row, truncated, *style, available as u16);
            cur_col += truncated.chars().count() as u16;
        }
        // Pad remainder of line
        let used = (cur_col - (col + 1)) as usize;
        if used < inner_w {
            let blanks = " ".repeat(inner_w - used);
            scr.print(cur_col, screen_row, &blanks, (inner_w - used) as u16);
        }
    }
}