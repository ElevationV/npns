#![allow(dead_code)]

use std::borrow::Cow;
use std::fs::metadata;
use std::path::PathBuf;
use std::os::unix::fs::FileTypeExt;
use anyhow::Result;
use std::io::Stdout;
use crate::fs_info::file_system_info::{FileSys, StatusFlag};
use crate::fs_info::file_info::FileInfo;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Table, Row, Cell, TableState},
    Frame, Terminal,
};

#[derive(PartialEq, Clone, Copy)]
enum InputContext {
    None,
    NewFile,
    NewDir,
    Rename,
    ConfirmDelete,
    Search,
}

pub struct App {
    fs: FileSys,
    table_state: TableState, // cursor index
    input_context: InputContext,
    input_buffer: String,
    show_hidden: bool,
    search_query: String,
    should_quit: bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<App> {
        let app = App{
            fs: FileSys::init(start_dir)?,
            table_state: TableState::default(),
            input_context: InputContext::None,
            input_buffer: String::new(),
            show_hidden: false,
            search_query: String::new(),
            should_quit: false,
        };
        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|frame| self.ui(frame))?;

            if self.should_quit {
                return Ok(())
            }
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    let _ = self.handle_key(key.code);
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        if self.input_context != InputContext::None {
            self.handle_input_mode(key)
        } else {
            self.handle_normal_mode(key)
        }
    }

    ///
    /// # Key Handler in Input Mod
    ///

    fn handle_input_mode(&mut self, key: KeyCode) -> Result<()> {
        match key {
            KeyCode::Char(c) => self.input_buffer.push(c),
            KeyCode::Backspace => {self.input_buffer.pop();},
            KeyCode::Enter => self.submit_input()?,
            KeyCode::Esc => self.exit_input_mode(),
            _ => {}
        }

        Ok(())
    }

    fn submit_input(&mut self) -> Result<()> {
        let input = self.input_buffer.trim().to_string();

        if self.input_context == InputContext::Search {
            self.search_query = input;
            self.reset_cursor();
            self.clear_selection();
            self.exit_input_mode();
            return Ok(());
        }
        if self.input_context == InputContext::ConfirmDelete {
            if input == 'y'.to_string() || input == 'Y'.to_string() {
                self.fs.delete_selected()?;
                self.exit_input_mode();
            } else if input == 'n'.to_string() || input == 'N'.to_string() {
                self.exit_input_mode();
            }

            return Ok(());
        }

        if !input.is_empty() {
            let result = match self.input_context {
                InputContext::NewFile => self.fs.new_file(&input, false),
                InputContext::NewDir => self.fs.new_file(&input, true),
                InputContext::Rename => self.fs.rename_selected(&input),
                _ => Ok(())
            };

            if let Err(error) = result {
                self.fs.status_info = format!("Error: {}", error);
                self.fs.status_flag = StatusFlag::Error;
            }
        }

        self.exit_input_mode();
        Ok(())
    }

    // clear input buffer and flags
    fn exit_input_mode(&mut self) {
        self.input_context = InputContext::None;
        self.input_buffer.clear();
        self.fs.status_info = "Ready".to_string();
        self.fs.status_flag = StatusFlag::Ready;
    }

    ///
    /// # Key Handler in Normal Mode
    ///
    fn handle_normal_mode(&mut self, key: KeyCode) -> Result<()> {
        match key {
            // guide
            KeyCode::Char('j') => self.move_cursor(-1),
            KeyCode::Char('k') => self.move_cursor(1),
            KeyCode::Char('h') => self.go_parent_dir(),
            KeyCode::Char('l') => self.enter_current(),

            // selection
            KeyCode::Char(' ') => self.toggle_selection(),

            // file operations
            KeyCode::Char('c') => self.fs.copy_selected(true),
            KeyCode::Char('x') => self.fs.copy_selected(false),
            KeyCode::Char('v') => self.fs.paste(),
            KeyCode::Char('d') => self.start_delete_confirm(),
            KeyCode::Char('u') => self.fs.undo(),
            KeyCode::Char('r') => self.start_rename(),

            // create
            KeyCode::Char('n') => self.start_new_file(),
            KeyCode::Char('m') => self.start_new_dir(),

            // filter or search
            KeyCode::Char('.') => self.toggle_hidden_files(),
            KeyCode::Char('/') => self.start_search(),
            KeyCode::Esc => self.clear_search(),

            // exit
            KeyCode::Char('q') => {
                self.should_quit = true;
                Ok(())
            }

            _ => Ok(())
        }
    }

    ///
    /// # Guide
    ///
    fn move_cursor(&mut self, delta: i32) -> Result<()> {
        let len = self.filtered_files().len();
        if len == 0 {
            self.table_state.select(None);
            return Ok(())
        }

        let new_index = match self.table_state.selected() {
            Some(i) => {
                if delta > 0 {
                    if i >= len - 1 { 0 } else { i + 1 }
                } else {
                    if i == 0 { len - 1 } else { i - 1 }
                }
            },
            None => 0,
        };

        self.table_state.select(Some(new_index));
        Ok(())
    }

    fn go_parent_dir(&mut self) -> Result<()> {
        self.fs.parent_dir()?;
        self.clear_selection(); // clear selection
        self.reset_cursor();    // clear cursor
        Ok(())
    }

    fn enter_current(&mut self) -> Result<()> {
        if let Some((original_index, is_dir)) = self.get_cursor_file_info() {
            if is_dir {
                self.fs.select_current(original_index);
                self.fs.sub_dir(original_index)?;

                self.search_query.clear();
                self.clear_selection();
                self.reset_cursor();
            }
        }

        Ok(())
    }

    ///
    /// # Select Operation
    ///
    fn toggle_selection(&mut self) -> Result<()> {
        if let Some((original_index, _)) = self.get_cursor_file_info() {
            if self.fs.selected_index() == Some(original_index) {
                self.fs.selected_index = None;
            } else {
                self.fs.selected_index = Some(original_index);
            }
        }
        Ok(())
    }
    fn clear_selection(&mut self){
        self.fs.selected_index = None;
    }
    fn reset_cursor(&mut self) {
        // if nothing in current dir(after search), current index should be None
        let filtered = self.filtered_files();
        self.table_state.select(if filtered.is_empty() {None} else {Some(0)});
    }

    ///
    /// # File Operation
    ///
    fn start_delete_confirm(&mut self) -> Result<()> {
        if self.fs.selected_index.is_some(){
            self.input_context = InputContext::ConfirmDelete;
        } else {
            self.exit_input_mode()
        }
        Ok(())
    }

    fn start_rename(&mut self) -> Result<()> {
        if let Some((original_index, _)) = self.get_cursor_file_info() {
            if let Some(file) = self.fs.files().clone().get(original_index) {
                self.fs.selected_index = Some(original_index);
                self.input_buffer = file.name.clone();
                self.input_context = InputContext::Rename;
            }
        }
        Ok(())
    }

    fn start_new_file(&mut self) -> Result<()> {
        self.input_context = InputContext::NewFile;
        self.input_buffer.clear();
        Ok(())
    }

    fn start_new_dir(&mut self) -> Result<()> {
        self.input_context = InputContext::NewDir;
        self.input_buffer.clear();
        Ok(())
    }

    ///
    /// # Search
    ///
    fn toggle_hidden_files(&mut self) -> Result<()> {
        self.show_hidden = !self.show_hidden; // toggle status
        self.search_query.clear();      // clear search buffer
        self.reset_cursor();
        Ok(())
    }

    fn start_search(&mut self) -> Result<()> {
        self.input_context = InputContext::Search;
        self.input_buffer.clear(); // set input flag
        self.reset_cursor(); // clean search buffer
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.reset_cursor();
        }
        Ok(())
    }

    ///
    /// # UI
    ///
    fn ui(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area());

        self.render_table(frame, chunks[0]);
        self.render_status_bar(frame, chunks[1]);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        // only show filtered files
        let table = self.filtered_files();

        let rows: Vec<Row> = table.iter().map(|(index, file)| {
            let style = if Some(*index) == self.fs.selected_index(){
                Style::default().add_modifier(Modifier:: BOLD).fg(Color::Cyan) // selected
            } else {
                Style::default() // not selected
            };

            Row::new(vec![
                Cell::from(file.name.clone()),
                Cell::from(if file.is_dir{"-".to_string()} else { format_file_size(file.size) }),
                Cell::from(get_file_type(&file.path)),
            ]).style(style)
        }).collect();// [file_name, file_size, file_type] + style(for selected)

        let mut title = self.fs.current_dir().display().to_string();
        if !self.search_query.is_empty() { // when searching, title should change
            title = format!("{} [Searching: '{}']", title, self.search_query);
        }

        let table = Table::new(rows, [Constraint::Min(30), Constraint::Length(12), Constraint::Min(6)])
            .header(Row::new(vec!["Name", "Size", "Type"]).style(Style::default().add_modifier(Modifier::BOLD)))
            .block(Block::default().borders(Borders::ALL).title(title))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .column_spacing(1);

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_status_bar(&mut self, frame: &mut Frame, area: Rect) {
        let (title, content, color) = match self.input_context {
            InputContext::Search =>
                ("Search", Cow::Borrowed(self.input_buffer.as_str()), Color::Gray),
            InputContext::ConfirmDelete =>
                ("Confirm", Cow::Owned(format!("Removed files cannot recover (y/N): {}", self.input_buffer)), Color::Magenta),
            InputContext::None => {
                let mut text = self.fs.status_info.clone();
                if !self.search_query.is_empty() {
                    text = format!("{} | Search: '{}'", text, self.search_query);
                }
                if self.show_hidden {
                    text = format!("{} | [Hidden Shown]", text);
                }

                let color = match self.fs.status_flag {
                    StatusFlag::Error => Color::Red,
                    StatusFlag::Ready => Color::Green,
                    StatusFlag::Input => Color::Yellow,
                    _ => Color::White,
                };
                ("Status", Cow::Owned(text), color)
            }
            _ => ("Input", Cow::Borrowed(self.input_buffer.as_str()), Color::Yellow),
        };

        let widget = Paragraph::new(content.as_ref())
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().fg(color));
        frame.render_widget(widget, area);
    }

    ///
    /// # Helpers
    ///

    fn filtered_files(&self) -> Vec<(usize, &FileInfo)> { // (original_index, file_info)
        // filter files, include hide and search
        self.fs.files()
            .iter()
            .enumerate() // original index
            .filter(|(_, file)| {
                // hide
                let show_file = self.show_hidden || !file.name.starts_with('.');
                // search
                let matches_search = self.search_query.is_empty()
                    || file.name.to_lowercase().contains(&self.search_query.to_lowercase());
                show_file && matches_search
            })
            .collect()
    }

    fn get_cursor_file_info(&self) -> Option<(usize, bool)> { // (original_index, is_dir)
        let filtered = self.filtered_files(); // (original_index, file_info)
        self.table_state.selected()
            .and_then(|index| {filtered.get(index)})
            .map(|(original_index, file)| (*original_index, file.is_dir))
    }
}

fn format_file_size(size: u64) -> String {
    if size == 0 { return "0 B".to_string(); }

    let units = ["B", "KB", "MB", "GB"];
    let mut value = size as f64;
    let mut unit_idx = 0;

    while value >= 1024.0 && unit_idx < units.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.1} {}", value, units[unit_idx])
}

fn get_file_type(path: &PathBuf) -> &'static str {
    if let Ok(metadata) = metadata(path) {
        let file_type = metadata.file_type();

        if file_type.is_dir() { "DIR" }
        else if file_type.is_file() { "FILE" }
        else if file_type.is_symlink() { "SYMLINK" }
        else if file_type.is_fifo() { "FIFO" }
        else if file_type.is_char_device() { "CHAR" }
        else if file_type.is_block_device() { "BLOCK" }
        else if file_type.is_socket() { "SOCKET" }
        else { "UNKNOWN" }
    } else {
        "ERROR"
    }
}
