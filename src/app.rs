#![allow(dead_code)]

use std::fs::metadata;
use std::io::Read;
use std::path::PathBuf;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::fs::OpenOptionsExt;

use crate::fs_info::{FileSystemCore, DuplicatedFileHandleOps, StateFlag};
use crate::ui::{
    read_key, KeyCode,
    Color, Rect, Screen, Style,
    ColWidth, DialogLine, Row,
    render_dialog, render_list, render_paragraph,
    render_status_bar, render_table,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct CursorInfo {
    orig_idx: Option<usize>,
    pos:      usize, 
    total:    usize,
}

// Input context
#[derive(PartialEq, Clone, Copy)]
enum InputContext {
    None,
    NewFile,
    NewDir,
    Rename,
    ConfirmDelete,
    Search,
}

// Duplicate dialog state
#[derive(PartialEq, Clone)]
enum DuplicateDialogMode {
    File,
    Dir,
}

#[derive(Clone)]
struct DuplicateDialog {
    path:         PathBuf,
    mode:         DuplicateDialogMode,
    apply_to_all: bool,
    cursor:       usize,
    rename_input: Option<String>,
}

impl DuplicateDialog {
    fn new(path: PathBuf, is_dir: bool) -> Self {
        DuplicateDialog {
            path,
            mode: if is_dir { DuplicateDialogMode::Dir } else { DuplicateDialogMode::File },
            apply_to_all: false,
            cursor: 0,
            rename_input: None,
        }
    }

    fn options(&self) -> &'static [&'static str] {
        match self.mode {
            DuplicateDialogMode::File => &["Overwrite", "Rename", "Skip", "Cancel"],
            DuplicateDialogMode::Dir  => &["Write In",  "Rename", "Skip", "Cancel"],
        }
    }
}

// App
pub struct App {
    fs:             FileSystemCore,
    cursor:         usize,         // index into filtered_files()
    selected_index: Option<usize>, // marked file (orig index into fs.files())
    input_context:  InputContext,
    input_buffer:   String,
    show_hidden:    bool,
    search_query:   String,
    should_quit:    bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<App> {
        Ok(App {
            fs:             FileSystemCore::init(start_dir),
            cursor:         0,
            selected_index: None,
            input_context:  InputContext::None,
            input_buffer:   String::new(),
            show_hidden:    false,
            search_query:   String::new(),
            should_quit:    false,
        })
    }

    // Main loop
    pub fn run(&mut self, scr: &mut Screen) -> Result<()> {
        self.reset_cursor();

        loop {
            self.draw(scr);

            if self.should_quit { break; }

            let key = read_key()?;
            if key == KeyCode::Char('v') && self.input_context == InputContext::None {
                self.paste(scr);
            } else {
                self.handle_key(key);
            }
        }
        Ok(())
    }

    // Key dispatch
    fn handle_key(&mut self, key: KeyCode) {
        if self.input_context != InputContext::None {
            self.handle_input_mode(key);
        } else {
            self.handle_normal_mode(key);
        }
    }

    // Input mode
    fn handle_input_mode(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char(c)   => { self.input_buffer.push(c); }
            KeyCode::Backspace => { self.input_buffer.pop(); }
            KeyCode::Enter     => self.submit_input(),
            KeyCode::Esc       => self.exit_input_mode(),
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let input = self.input_buffer.trim().to_string();

        match self.input_context {
            InputContext::Search => {
                self.search_query = input;
                self.reset_cursor();
                self.selected_index = None;
                self.exit_input_mode();
            }
            InputContext::ConfirmDelete => {
                if input.eq_ignore_ascii_case("y") {
                    if let Some(idx) = self.selected_index {
                        let _ = self.fs.select(idx);
                    }
                    self.fs.remove_selected();
                    self.selected_index = None;
                    self.reset_cursor();
                }
                self.exit_input_mode();
            }
            InputContext::NewFile => {
                if !input.is_empty() {
                    self.fs.new_file(&input, false);
                    self.reset_cursor();
                }
                self.exit_input_mode();
            }
            InputContext::NewDir => {
                if !input.is_empty() {
                    self.fs.new_file(&input, true);
                    self.reset_cursor();
                }
                self.exit_input_mode();
            }
            InputContext::Rename => {
                if !input.is_empty() {
                    if let Some(idx) = self.selected_index {
                        let _ = self.fs.select(idx);
                    }
                    self.fs.rename_selected(&input);
                    self.selected_index = None;
                    self.reset_cursor();
                }
                self.exit_input_mode();
            }
            InputContext::None => {}
        }
    }

    fn exit_input_mode(&mut self) {
        self.input_context = InputContext::None;
        self.input_buffer.clear();
    }

    // Normal mode
    fn handle_normal_mode(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('j')                  => self.move_cursor(1),
            KeyCode::Char('k')                  => self.move_cursor(-1),
            KeyCode::Char('h')                  => self.go_parent_dir(),
            KeyCode::Char('l') | KeyCode::Enter => self.enter_current(),

            #[cfg(debug_assertions)]
            KeyCode::Down  => self.move_cursor(1),
            #[cfg(debug_assertions)]
            KeyCode::Up    => self.move_cursor(-1),
            #[cfg(debug_assertions)]
            KeyCode::Left  => self.go_parent_dir(),
            #[cfg(debug_assertions)]
            KeyCode::Right => self.enter_current(),

            KeyCode::Char(' ') => self.toggle_selection(),

            KeyCode::Char('c') => self.copy_marked(true),
            KeyCode::Char('x') => self.copy_marked(false),
            KeyCode::Char('d') => self.start_delete_confirm(),
            KeyCode::Char('u') => self.fs.undo(),
            KeyCode::Char('r') => self.start_rename(),

            KeyCode::Char('n') => {
                self.input_context = InputContext::NewFile;
                self.input_buffer.clear();
            }
            KeyCode::Char('m') => {
                self.input_context = InputContext::NewDir;
                self.input_buffer.clear();
            }

            KeyCode::Char('.') => self.toggle_hidden(),
            KeyCode::Char('/') => {
                self.input_context = InputContext::Search;
                self.input_buffer.clear();
            }
            KeyCode::Esc => self.clear_search(),
            KeyCode::Char('q') => { self.should_quit = true; }

            _ => {}
        }
    }

    // Navigation
    fn move_cursor(&mut self, delta: i32) {
        let len = self.filtered_files().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        if delta > 0 {
            self.cursor = if self.cursor + 1 >= len { 0 } else { self.cursor + 1 };
        } else {
            self.cursor = if self.cursor == 0 { len - 1 } else { self.cursor - 1 };
        }
    }

    fn go_parent_dir(&mut self) {
        self.fs.parent_dir();
        self.selected_index = None;
        self.reset_cursor();
    }

    fn enter_current(&mut self) {
        if let Some((orig_idx, true)) = self.cursor_file_info() {
            let _ = self.fs.select(orig_idx);
            self.fs.enter_selected();
            self.search_query.clear();
            self.selected_index = None;
            self.reset_cursor();
        }
    }

    // Selection / marking
    fn toggle_selection(&mut self) {
        if let Some((orig_idx, _)) = self.cursor_file_info() {
            if self.selected_index == Some(orig_idx) {
                self.selected_index = None;
            } else {
                self.selected_index = Some(orig_idx);
            }
        }
    }

    // File operations
    fn copy_marked(&mut self, is_copy: bool) {
        if let Some(idx) = self.selected_index {
            let _ = self.fs.select(idx);
            self.fs.copy_selected(is_copy);
        }
    }

    fn paste(&mut self, scr: &mut Screen) {
        let scr_ptr  = scr  as *mut Screen;
        let self_ptr = self as *mut App;

        self.fs.paste(move |path, is_dir| {
            let scr  = unsafe { &mut *scr_ptr };
            let app  = unsafe { &mut *self_ptr };

            let mut dialog = DuplicateDialog::new(path.clone(), is_dir);

            loop {
                app.draw(scr);
                app.draw_duplicate_dialog(scr, &dialog);
                scr.present();

                let key = match read_key() {
                    Ok(k)  => k,
                    Err(_) => return (DuplicatedFileHandleOps::Cancel, true),
                };

                // Rename sub-mode: collect the new name
                if dialog.rename_input.is_some() {
                    match key {
                        KeyCode::Char(c)   => { dialog.rename_input.as_mut().unwrap().push(c); }
                        KeyCode::Backspace => { dialog.rename_input.as_mut().unwrap().pop(); }
                        KeyCode::Enter     => {
                            return (Self::handler_from_dialog(&dialog), dialog.apply_to_all);
                        }
                        KeyCode::Esc       => { dialog.rename_input = None; }
                        _ => {}
                    }
                    continue;
                }

                // Normal dialog navigation
                match key {
                    KeyCode::Char('j') | KeyCode::Down => {
                        if dialog.cursor + 1 < dialog.options().len() {
                            dialog.cursor += 1;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if dialog.cursor > 0 { dialog.cursor -= 1; }
                    }
                    KeyCode::Char('a') | KeyCode::Char(' ') => {
                        dialog.apply_to_all = !dialog.apply_to_all;
                    }
                    KeyCode::Enter => {
                        if dialog.cursor == 1 {
                            let default = path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            dialog.rename_input = Some(default);
                        } else {
                            return (Self::handler_from_dialog(&dialog), dialog.apply_to_all);
                        }
                    }
                    KeyCode::Esc => {
                        return (DuplicatedFileHandleOps::Cancel, true);
                    }
                    _ => {}
                }
            }
        });

        self.reset_cursor();
    }

    fn handler_from_dialog(dialog: &DuplicateDialog) -> DuplicatedFileHandleOps {
        match dialog.cursor {
            0 => match dialog.mode {
                DuplicateDialogMode::File => DuplicatedFileHandleOps::Overwrite,
                DuplicateDialogMode::Dir  => DuplicatedFileHandleOps::WriteIn,
            },
            1 => DuplicatedFileHandleOps::Rename(
                dialog.rename_input.clone().unwrap_or_default()
            ),
            2 => DuplicatedFileHandleOps::Skip,
            _ => DuplicatedFileHandleOps::Cancel,
        }
    }

    fn start_delete_confirm(&mut self) {
        if self.selected_index.is_some() {
            self.input_context = InputContext::ConfirmDelete;
            self.input_buffer.clear();
        }
    }

    fn start_rename(&mut self) {
        let Some(orig_idx) = self.selected_index else { return };
        if let Some(path) = self.fs.get_file(orig_idx) {
            self.input_buffer = path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.input_context = InputContext::Rename;
        }
    }

    // Search / filter
    fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.search_query.clear();
        self.reset_cursor();
    }

    fn clear_search(&mut self) {
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.reset_cursor();
        }
    }

    // Helpers 
    fn filtered_files(&self) -> Vec<(usize, &PathBuf)> {
        self.fs
            .files()
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let name = f.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let visible = self.show_hidden || !name.starts_with('.');
                let matched = self.search_query.is_empty()
                    || name.to_lowercase().contains(&self.search_query.to_lowercase());
                visible && matched
            })
            .collect()
    }

    fn cursor_file_info(&self) -> Option<(usize, bool)> {
        let filtered = self.filtered_files();
        filtered.get(self.cursor).map(|(orig, path)| (*orig, path.is_dir()))
    }

    fn reset_cursor(&mut self) {
        let len = self.filtered_files().len();
        self.cursor = if len == 0 { 0 } else { self.cursor.min(len - 1) };
    }

    // Drawing
    // Full-frame render.  Called every event loop iteration.
    fn draw(&mut self, scr: &mut Screen) {
        scr.resize();
        scr.clear_all();

        let cols = scr.cols;
        let rows = scr.rows;
        let left_w     = cols / 2;
        let right_w    = cols - left_w;
        let pane_h     = rows.saturating_sub(3);
        let status_row = rows.saturating_sub(2);
        let filtered   = self.filtered_files();
        let total      = filtered.len();

        let cursor = CursorInfo {
            orig_idx: filtered.get(self.cursor).map(|(i, _)| *i),
            pos:      if total == 0 { 0 } else { self.cursor + 1 },
            total,
        };
        
        self.draw_file_list(scr, 1, 1, left_w, pane_h);
        self.draw_preview(scr, left_w + 1, 1, right_w, pane_h);
        self.draw_status_bar(scr, 1, status_row, cols, cursor);

        scr.present();
    }

    // Left pane: file list
    fn draw_file_list(&mut self, scr: &mut Screen, col: u16, row: u16, w: u16, h: u16) {
        let filtered = self.filtered_files();

        // Title: current directory path, with optional search query suffix
        let mut title = self.fs.current_dir().display().to_string();
        if !self.search_query.is_empty() {
            title = format!("{} [/{}]", title, self.search_query);
        }

        // Build header row
        let header = Row::new(vec![" ", "Name", "Size", "Type"])
            .style(Style::new().fg(Color::Yellow).bold());

        // Column layout: marker(2) | name(fill) | size(10) | type(7)
        let col_widths = [
            ColWidth::Fixed(2),
            ColWidth::Fill,
            ColWidth::Fixed(10),
            ColWidth::Fixed(7),
        ];

        // Build data rows
        let rows_data: Vec<Row> = filtered
            .iter()
            .map(|(orig_idx, path)| {
                let is_marked = self.selected_index == Some(*orig_idx);

                let mark_str = if is_marked { ">" } else { " " };
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let size_str = if path.is_dir() {
                    "  —".to_string()
                } else {
                    path.metadata()
                        .map(|m| format_size(m.len()))
                        .unwrap_or_else(|_| "?".to_string())
                };
                let type_str = file_type(path);

                let name_style = if path.is_dir() {
                    Style::new().fg(Color::Blue).bold()
                } else {
                    Style::new()
                };
                let mark_style = if is_marked {
                    Style::new().fg(Color::Cyan)
                } else {
                    Style::new()
                };

                Row::new(vec![mark_str, name.as_str(), size_str.as_str(), type_str])
                    .cell_style(0, mark_style)
                    .cell_style(1, name_style)
            })
            .collect();

        render_table(
            scr,
            Rect::new(col, row, w, h),
            &title,
            &header,
            &rows_data,
            &col_widths,
            Some(self.cursor),
        );
    }

    // Right pane: preview

    fn draw_preview(&self, scr: &mut Screen, col: u16, row: u16, w: u16, h: u16) {
        let area = Rect::new(col, row, w, h);

        let Some((orig_idx, is_dir)) = self.cursor_file_info() else {
            render_paragraph(scr, area, "Preview", "(empty)", Style::new(), Style::new());
            return;
        };

        let Some(file) = self.fs.get_file(orig_idx) else {
            render_paragraph(scr, area, "Preview", "(no file)", Style::new(), Style::new());
            return;
        };

        let title = file.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if is_dir {
            self.draw_dir_preview(scr, area, file, &title);
        } else {
            self.draw_file_preview(scr, area, file, &title);
        }
    }

    /// Preview for a regular file.
    ///
    /// Text files (valid UTF-8 header): show file content.
    /// Binary files: show file-type hint from extension + `<binary>`.
    /// Symlinks: show the link target.
    fn draw_file_preview(&self, scr: &mut Screen, area: Rect, file: &PathBuf, title: &str) {
        let size_str = file.metadata()
            .map(|m| format_size(m.len()))
            .unwrap_or_else(|_| "?".to_string());

        let meta = file.symlink_metadata().ok();
        let is_regular = meta.as_ref()
            .map(|m| m.file_type().is_file())
            .unwrap_or(false);
        
        let mut buf = [0u8; 512];
        let n = if is_regular {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(file)
                .and_then(|mut f| f.read(&mut buf))
                .unwrap_or(0)
        } else {
            0
        };

        let content = if file.is_symlink() {
            let target = std::fs::read_link(file)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".to_string());
            format!("Size: {}\nsymlink -> {}", size_str, target)
        } else if n == 0 || std::str::from_utf8(&buf[..n]).is_ok() {
            // Valid UTF-8 — treat as text and show the full content
            let inner_h = area.h.saturating_sub(4) as usize;
            let text = read_text_preview(file, inner_h);
            format!("Size: {}\n\n{}", size_str, text)
        } else {
            // Binary — show extension-based description
            let ext = file.extension()
                .and_then(|e| e.to_str())
                .map(|e| format!("{} file", e))
                .unwrap_or_else(|| "binary".to_string());
            format!("Size: {}\n{}\n<binary>", size_str, ext)
        };

        render_paragraph(
            scr, area, title,
            &content,
            Style::new().fg(Color::Gray),
            Style::new().fg(Color::DarkGray),
        );
    }

    fn draw_dir_preview(
        &self,
        scr:   &mut Screen,
        area:  Rect,
        dir:   &PathBuf,
        title: &str,
    ) {
        use std::fs;

        let items: Vec<(String, Style)> = match fs::read_dir(dir) {
            Err(_) => {
                render_paragraph(
                    scr, area, title,
                    "(permission denied)", Style::new(), Style::new(),
                );
                return;
            }
            Ok(rd) => {
                let mut entries: Vec<(String, bool)> = rd
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if !self.show_hidden && name.starts_with('.') { return None; }
                        Some((name, is_dir))
                    })
                    .collect();

                entries.sort_by(|(na, da), (nb, db)| {
                    match (da, db) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => na.cmp(nb),
                    }
                });

                entries.into_iter().map(|(name, is_dir)| {
                    let prefix = if is_dir { "> " } else { "  " };
                    let style = if is_dir {
                        Style::new().fg(Color::Blue).bold()
                    } else {
                        Style::new().fg(Color::Gray)
                    };
                    (format!("{}{}", prefix, name), style)
                }).collect()
            }
        };

        render_list(
            scr, area, title,
            &items,
            Style::new().fg(Color::Blue),
        );
    }

    // Status bar
    fn draw_status_bar(
        &self,
        scr:    &mut Screen,
        col:    u16,
        row:    u16,
        w:      u16,
        cursor: CursorInfo,
    ) {
        // Left: state / prompt
        let (title, left_content, color) = match self.input_context {
            InputContext::Search => (
                "Search",
                format!("{}_", self.input_buffer),
                Color::Cyan,
            ),
            InputContext::ConfirmDelete => (
                "Delete",
                format!("Delete marked file? (y/N): {}", self.input_buffer),
                Color::Red,
            ),
            InputContext::NewFile => (
                "New File",
                format!("Name: {}_", self.input_buffer),
                Color::Yellow,
            ),
            InputContext::NewDir => (
                "New Dir",
                format!("Name: {}_", self.input_buffer),
                Color::Yellow,
            ),
            InputContext::Rename => (
                "Rename",
                format!("New name: {}_", self.input_buffer),
                Color::Yellow,
            ),
            InputContext::None => {
                let flag = self.fs.state_flag();
                let info = self.fs.state_info().to_string();
                let mut parts = vec![info];
                if !self.search_query.is_empty() {
                    parts.push(format!("search: '{}'", self.search_query));
                }
                if self.show_hidden {
                    parts.push("hidden shown".to_string());
                }
                if let Some(idx) = self.selected_index
                    && let Some(f) = self.fs.get_file(idx)
                {
                    let name = f.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    parts.push(format!("marked: {}", name));
                }
                let color = match flag {
                    StateFlag::Error     => Color::Red,
                    StateFlag::Ready     => Color::Green,
                    StateFlag::Operating => Color::Yellow,
                };
                ("Status", parts.join("  │  "), color)
            }
        };

        // Right: permissions + position
        // Use symlink_metadata so symlinks show 'l' instead of following the link.
        let perms = cursor.orig_idx
            .and_then(|idx| self.fs.get_file(idx))
            .and_then(|path| path.symlink_metadata().ok())
            .map(|m| format_permissions(m.permissions().mode()))
            .unwrap_or_else(|| "----------".to_string());

        let right = format!("{}  {}/{}", perms, cursor.pos, cursor.total);

        // Draw border + left content first
        render_status_bar(scr, col, row, w, title, &left_content, Style::new().fg(color));

        // Overwrite the right portion of the inner row with the right-aligned string.
        // Inner area: columns [col+1 .. col+w-1], row+1.
        let inner_w   = w.saturating_sub(2) as usize;
        let right_len = right.len().min(inner_w);
        let right_col = col + 1 + (inner_w - right_len) as u16;
        scr.print_styled(
            right_col, row + 1,
            &right,
            Style::new().fg(Color::DarkGray),
            right_len as u16,
        );
    }

    // Duplicate dialog
    fn draw_duplicate_dialog(&self, scr: &mut Screen, dialog: &DuplicateDialog) {
        let filename = dialog.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dialog.path.display().to_string());

        let title = if dialog.mode == DuplicateDialogMode::File {
            " Duplicate File "
        } else {
            " Duplicate Directory "
        };

        let border_style = Style::new().fg(Color::Red).bold();
        let mut lines: Vec<DialogLine> = Vec::new();

        // " <filename> already exists"
        lines.push(
            DialogLine::plain("  ")
                .push(&filename, Style::new().fg(Color::Yellow).bold())
                .push(" already exists", Style::new()),
        );
        lines.push(DialogLine::empty());

        if dialog.rename_input.is_none() {
            // Apply-to-all toggle
            let toggle = if dialog.apply_to_all { "[*]" } else { "[ ]" };
            lines.push(
                DialogLine::plain("  ")
                    .push(toggle, Style::new().fg(Color::Cyan))
                    .push("  Apply to all  ", Style::new())
                    .push("(a / space)", Style::new().fg(Color::DarkGray)),
            );
            lines.push(DialogLine::empty());

            // Option list
            for (i, label) in dialog.options().iter().enumerate() {
                let selected = i == dialog.cursor;
                let marker = if selected { "> " } else { "  " };
                let style = if selected {
                    Style::new().fg(Color::White).bold().reverse()
                } else {
                    Style::new().fg(Color::Gray)
                };
                lines.push(
                    DialogLine::plain("  ")
                        .push(format!("{}{}", marker, label), style),
                );
            }
        } else {
            let name = dialog.rename_input.as_deref().unwrap_or("");
            lines.push(
                DialogLine::plain("  Rename to: ")
                    .push(format!("{}_", name), Style::new().fg(Color::White).bold()),
            );
        }

        // Dialog height: border(2) + "exists" line + blank + toggle section + options
        let options_h = if dialog.rename_input.is_none() {
            dialog.options().len() as u16 + 2 // toggle line + blank
        } else {
            1
        };
        let dialog_h = 2 + 2 + options_h;

        render_dialog(scr, 52, dialog_h, title, &lines, border_style);
    }
}

// Utility functions

fn format_size(size: u64) -> String {
    if size == 0 { return "0 B".to_string(); }
    let units = ["B", "KB", "MB", "GB"];
    let mut value = size as f64;
    let mut idx   = 0usize;
    while value >= 1024.0 && idx < units.len() - 1 {
        value /= 1024.0;
        idx   += 1;
    }
    format!("{:.1} {}", value, units[idx])
}

fn file_type(path: &PathBuf) -> &'static str {
    match metadata(path) {
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

// Format a Unix mode bitmask as a `drwxrwxrwx`-style 10-character string.
fn format_permissions(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd', // directory
        0o120000 => 'l', // symlink
        0o060000 => 'b', // block device
        0o020000 => 'c', // char device
        0o010000 => 'p', // fifo / named pipe
        0o140000 => 's', // socket
        _        => '-', // regular file
    };

    let bits = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'), // owner
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'), // group
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'), // other
    ];

    let mut s = String::with_capacity(10);
    s.push(kind);
    for (bit, ch) in bits {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}

fn read_text_preview(path: &PathBuf, max_lines: usize) -> String {
    use std::io::{BufRead, BufReader};
        use std::os::unix::fs::OpenOptionsExt;
    
        let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        else {
            return String::new();
        };
    
        let mut reader = BufReader::new(file.take(64 * 1024));
    let mut out = String::new();
    let mut line = String::new();
    let mut count = 0;

    while count < max_lines {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,           // EOF
            Ok(_) => {
                out.push_str(&line);
                count += 1;
            }
            Err(_) => break,
        }
    }
    out
}