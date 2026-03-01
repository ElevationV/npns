#![allow(dead_code)]

use std::fs::metadata;
use std::io::Stdout;
use std::path::PathBuf;
use std::os::unix::fs::FileTypeExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Cell, Table, TableState},
    Frame, Terminal,
};

use crate::fs_info::{
    FileSystemCore,
    DuplicatedFileHandleOps,
    StateFlag,
    FileInfo,
};

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

// Duplicate dialog

#[derive(PartialEq, Clone)]
enum DuplicateDialogMode {
    File,
    Dir,
}

#[derive(Clone)]
struct DuplicateDialog {
    path: PathBuf,
    mode: DuplicateDialogMode,
    apply_to_all: bool,
    cursor: usize,
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
    fs: FileSystemCore,

    table_state: TableState,
    selected_index: Option<usize>,

    input_context: InputContext,
    input_buffer: String,

    show_hidden: bool,
    search_query: String,

    should_quit: bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<App> {
        Ok(App {
            fs: FileSystemCore::init(start_dir),
            table_state: TableState::default(),
            selected_index: None,
            input_context: InputContext::None,
            input_buffer: String::new(),
            show_hidden: false,
            search_query: String::new(),
            should_quit: false,
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        self.reset_cursor();

        loop {
            terminal.draw(|frame| self.ui(frame))?;

            if self.should_quit {
                return Ok(());
            }

            if let Ok(Event::Key(key)) = event::read() && key.kind == KeyEventKind::Press {
                if key.code == KeyCode::Char('v')
                    && self.input_context == InputContext::None
                {
                    self.paste(terminal);
                } else {
                    self.handle_key(key.code);
                }
            }
        }
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

            KeyCode::Char('c') => self.copy_marked(true),  // copy 
            KeyCode::Char('x') => self.copy_marked(false), // cut
            // 'v' is intercepted in run() to pass terminal
            KeyCode::Char('d') => self.start_delete_confirm(),
            KeyCode::Char('u') => self.fs.undo(),
            KeyCode::Char('r') => self.start_rename(),

            KeyCode::Char('n') => { self.input_context = InputContext::NewFile; self.input_buffer.clear(); }
            KeyCode::Char('m') => { self.input_context = InputContext::NewDir;  self.input_buffer.clear(); }

            KeyCode::Char('.') => self.toggle_hidden(),
            KeyCode::Char('/') => { self.input_context = InputContext::Search; self.input_buffer.clear(); }
            KeyCode::Esc       => self.clear_search(),

            KeyCode::Char('q') => { self.should_quit = true; }

            _ => {}
        }
    }

    // Navigation

    fn move_cursor(&mut self, delta: i32) {
        let len = self.filtered_files().len();
        if len == 0 {
            self.table_state.select(None);
            return;
        }
        let new_index = match self.table_state.selected() {
            Some(i) => {
                if delta > 0 {
                    if i + 1 >= len { 0 } else { i + 1 }
                } else if i == 0 { len - 1 } else { i - 1 }
            }
            None => 0,
        };
        self.table_state.select(Some(new_index));
    }

    fn go_parent_dir(&mut self) {
        self.fs.parent_dir();
        self.selected_index = None;
        self.reset_cursor();
    }

    fn enter_current(&mut self) {
        if let Some((orig_idx, is_dir)) = self.cursor_file_info()
            && is_dir {
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


    fn paste(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
        let term_ptr = terminal as *mut Terminal<CrosstermBackend<Stdout>>;
        let self_ptr = self as *mut App;

        self.fs.paste(move |path, is_dir| {
            let terminal = unsafe { &mut *term_ptr };
            let app      = unsafe { &mut *self_ptr };

            let mut dialog = DuplicateDialog::new(path.clone(), is_dir);

            loop {
                terminal.draw(|frame| {
                    app.ui(frame);
                    Self::render_dialog(frame, &dialog);
                }).unwrap();

                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind != KeyEventKind::Press { continue; }

                    // Rename sub-mode
                    if dialog.rename_input.is_some() {
                        match key.code {
                            KeyCode::Char(c)   => { dialog.rename_input.as_mut().unwrap().push(c); }
                            KeyCode::Backspace => { dialog.rename_input.as_mut().unwrap().pop(); }
                            KeyCode::Enter => {
                                return (Self::handler_from_dialog(&dialog), dialog.apply_to_all);
                            }
                            KeyCode::Esc => { dialog.rename_input = None; }
                            _ => {}
                        }
                        continue;
                    }

                    // Normal dialog navigation
                    match key.code {
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
                                // Rename: collect name first
                                let default = path.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                dialog.rename_input = Some(default);
                            } else {
                                return (Self::handler_from_dialog(&dialog), dialog.apply_to_all);
                            }
                        }
                        KeyCode::Esc => {
                            // Cancel entire paste
                            return (DuplicatedFileHandleOps::Cancel, true);
                        }
                        _ => {}
                    }
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
        
        if let Some(file) = self.fs.get_file(orig_idx) {
            self.input_buffer = file.name().to_string();
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

    fn filtered_files(&self) -> Vec<(usize, &FileInfo)> {
        self.fs
            .files()
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let visible = self.show_hidden || !f.name().starts_with('.');
                let matched = self.search_query.is_empty()
                    || f.name().to_lowercase().contains(&self.search_query.to_lowercase());
                visible && matched
            })
            .collect()
    }

    fn cursor_file_info(&self) -> Option<(usize, bool)> {
        let filtered = self.filtered_files();
        self.table_state
            .selected()
            .and_then(|i| filtered.get(i))
            .map(|(orig, f)| (*orig, f.is_dir()))
    }

    fn reset_cursor(&mut self) {
        let filtered = self.filtered_files();
        self.table_state
            .select(if filtered.is_empty() { None } else { Some(0) });
    }

    // UI

    fn ui(&mut self, frame: &mut Frame) {
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area());

        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(outer[0]);

        self.render_file_list(frame, panes[0]);
        self.render_preview(frame, panes[1]);
        self.render_status_bar(frame, outer[1]);
    }

    // Duplicate dialog

    fn render_dialog(frame: &mut Frame, dialog: &DuplicateDialog) {
        let area = frame.area();
        let w = 52u16.min(area.width.saturating_sub(4));
        let rename_rows: u16 = if dialog.rename_input.is_some() { 2 } else { 0 };
        let h = (7 + dialog.options().len() as u16 + rename_rows)
            .min(area.height.saturating_sub(2));
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        let popup_area = Rect::new(x, y, w, h);

        frame.render_widget(Clear, popup_area);

        let filename = dialog.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dialog.path.display().to_string());

        let title = if dialog.mode == DuplicateDialogMode::File {
            "Duplicate File"
        } else {
            "Duplicate Directory"
        };

        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(&filename, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" already exists"),
        ]));
        lines.push(Line::raw(""));

        if dialog.rename_input.is_none() {
            let toggle = if dialog.apply_to_all { "[*]" } else { "[ ]" };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(toggle, Style::default().fg(Color::Cyan)),
                Span::raw("  Apply to all  "),
                Span::styled("(a / space)", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::raw(""));

            for (i, label) in dialog.options().iter().enumerate() {
                let selected = i == dialog.cursor;
                let marker = if selected { "> " } else { "  " };
                let style = if selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Gray)
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{marker}{label}"), style),
                ]));
            }
        } else {
            let name = dialog.rename_input.as_deref().unwrap_or("");
            lines.push(Line::from(vec![
                Span::raw("  Rename to: "),
                Span::styled(
                    format!("{name}_"),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                format!(" {title} "),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ))
            .title_alignment(Alignment::Center)
            .border_style(Style::default().fg(Color::Red));

        frame.render_widget(Paragraph::new(lines).block(block), popup_area);
    }

    // Left pane: file list

    fn render_file_list(&mut self, frame: &mut Frame, area: Rect) {
        let filtered = self.filtered_files();

        let rows: Vec<Row> = filtered
            .iter()
            .map(|(orig_idx, file)| {
                let is_marked = self.selected_index == Some(*orig_idx);
                let name_style = if file.is_dir() {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let mark_cell = if is_marked {
                    Cell::from(">").style(Style::default().fg(Color::Cyan))
                } else {
                    Cell::from(" ")
                };
                Row::new(vec![
                    mark_cell,
                    Cell::from(file.name().to_string()).style(name_style),
                    Cell::from(if file.is_dir() {
                        "  —".to_string()
                    } else {
                        format_size(file.size())
                    }),
                    Cell::from(file_type(file.path())),
                ])
            })
            .collect();

        let mut title = self.fs.current_dir().display().to_string();
        if !self.search_query.is_empty() {
            title = format!("{} [/{}]", title, self.search_query);
        }

        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Min(20),
                Constraint::Length(10),
                Constraint::Length(7),
            ],
        )
        .header(
            Row::new(vec![" ", "Name", "Size", "Type"])
                .style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow)),
        )
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    // Right pane: preview

    fn render_preview(&self, frame: &mut Frame, area: Rect) {
        let Some((orig_idx, is_dir)) = self.cursor_file_info() else {
            frame.render_widget(
                Paragraph::new("(empty)")
                    .block(Block::default().borders(Borders::ALL).title("Preview")),
                area,
            );
            return;
        };

        let Some(file) = self.fs.get_file(orig_idx) else {
            frame.render_widget(
                Paragraph::new("(no file)")
                    .block(Block::default().borders(Borders::ALL).title("Preview")),
                area,
            );
            return;
        };

        let title = file.name().to_string();

        if is_dir {
            self.render_dir_preview(frame, area, file.path(), &title);
        } else {
            let desc = self.fs.get_description(orig_idx);
            let text = desc.to_string_lossy().into_owned();
            let content = format!("Size: {}\n{}", format_size(file.size()), text);
            frame.render_widget(
                Paragraph::new(content)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    )
                    .style(Style::default().fg(Color::Gray)),
                area,
            );
        }
    }

    fn render_dir_preview(&self, frame: &mut Frame, area: Rect, dir: &PathBuf, title: &str) {
        use std::fs;

        let entries: Vec<ListItem> = match fs::read_dir(dir) {
            Err(_) => {
                frame.render_widget(
                    Paragraph::new("(permission denied)")
                        .block(Block::default().borders(Borders::ALL).title(title)),
                    area,
                );
                return;
            }
            Ok(rd) => {
                let mut items: Vec<(String, bool)> = rd
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        if !self.show_hidden && name.starts_with('.') { return None; }
                        Some((name, is_dir))
                    })
                    .collect();

                items.sort_by(|(na, da), (nb, db)| match (da, db) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => na.cmp(nb),
                });

                items.into_iter().map(|(name, is_dir)| {
                    let style = if is_dir {
                        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(if is_dir { "> " } else { "  " }, style),
                        Span::styled(name, style),
                    ]))
                }).collect()
            }
        };

        frame.render_widget(
            List::new(entries).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Blue)),
            ),
            area,
        );
    }

    // Status bar 

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let (title, content, color) = match self.input_context {
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
                if let Some(idx) = self.selected_index && let Some(f) = self.fs.get_file(idx) {
                    parts.push(format!("marked: {}", f.name()));
                }
                let color = match flag {
                    StateFlag::Error     => Color::Red,
                    StateFlag::Ready     => Color::Green,
                    StateFlag::Operating => Color::Yellow,
                };
                ("Status", parts.join("  │  "), color)
            }
        };

        frame.render_widget(
            Paragraph::new(content)
                .block(Block::default().borders(Borders::ALL).title(title))
                .style(Style::default().fg(color)),
            area,
        );
    }
}

// Utility functions

fn format_size(size: u64) -> String {
    if size == 0 { return "0 B".to_string(); }
    let units = ["B", "KB", "MB", "GB"];
    let mut value = size as f64;
    let mut idx = 0;
    while value >= 1024.0 && idx < units.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    format!("{:.1} {}", value, units[idx])
}

fn file_type(path: &PathBuf) -> &'static str {
    match metadata(path) {
        Err(_) => "ERR",
        Ok(m) => {
            let ft = m.file_type();
            if ft.is_dir()               { "DIR"  }
            else if ft.is_symlink()      { "LINK" }
            else if ft.is_fifo()         { "FIFO" }
            else if ft.is_char_device()  { "CHAR" }
            else if ft.is_block_device() { "BLK"  }
            else if ft.is_socket()       { "SOCK" }
            else                         { "FILE" }
        }
    }
}