#![allow(dead_code)]

use std::path::{PathBuf};
use anyhow::Result;
use std::io::{Stdout};
use crate::fs_info::file_system_info::{FileSys, StatusFlag};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
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
}

pub struct App {
    fs: FileSys,
    table_state: TableState,
    input_context: InputContext,
    input_buffer: String,
    should_quit: bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<Self> {
        let mut app = App {
            fs: FileSys::init(start_dir)?,
            table_state: TableState::default(),
            input_context: InputContext::None,
            input_buffer: String::new(),
            should_quit: false,
        };
        app.table_state.select(Some(0));
        Ok(app)
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;
            if self.should_quit {
                return Ok(());
            }
            match event::read() {
                Ok(Event::Key(key)) => {
                    if key.kind == KeyEventKind::Press {
                        let _ = self.handle_key(key.code);
                    }
                }
                Ok(Event::Mouse(_)) => {
                    continue;
                }
                Ok(Event::Resize(_, _)) => {
                    continue;
                }
                Ok(_) => {
                    continue;
                }
                Err(e) => {
                    self.fs.status_info = format!("Event error (ignored): {}", e);
                    self.fs.status_flag = StatusFlag::Others;
                    continue;
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        if self.input_context != InputContext::None {
            match key {
                KeyCode::Char('y') | KeyCode::Char('Y') if self.input_context == InputContext::ConfirmDelete => {
                    self.fs.delete_selected()?;
                    self.cancel_input();
                }

                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc if self.input_context == InputContext::ConfirmDelete => {
                    self.cancel_input();
                    self.fs.status_info = "Delete cancelled".to_string();
                }
                KeyCode::Char('q') => self.cancel_input(),
                KeyCode::Char(c) => self.input_buffer.push(c),
                KeyCode::Enter => {
                    let input = self.input_buffer.trim().to_string();
                    if !input.is_empty() {
                        let result = match self.input_context {
                            InputContext::NewFile => self.fs.new_file(&input, false),
                            InputContext::NewDir => self.fs.new_file(&input, true),
                            InputContext::Rename => self.fs.rename_selected(&input),
                            InputContext::ConfirmDelete => {
                                self.fs.delete_selected()?;
                                self.cancel_input();
                                return Ok(());
                            }
                            InputContext::None => Ok(()),
                        };

                        if let Err(e) = result {
                            self.fs.status_info = format!("Error: {}", e);
                            self.fs.status_flag = StatusFlag::Error;
                        }
                    }
                    self.cancel_input();
                }
                KeyCode::Backspace => { self.input_buffer.pop(); }
                _ => {}
            }
            return Ok(());
        }

        let current_idx = match self.table_state.selected() {
            Some(i) if i < self.fs.files().len() => i,
            _ => return Ok(()),
        };

        let _ = match key {
            KeyCode::Char('k') => self.next_row(),
            KeyCode::Char('j') => self.previous_row(),
            KeyCode::Char('l') | KeyCode::Enter => {
                self.fs.select_current(current_idx);
                if self.fs.files().get(current_idx).unwrap().is_dir {
                    self.fs.sub_dir(current_idx)?;
                    self.table_state.select(Some(0));
                    Ok(())
                } else {
                    Ok(())
                }
            }
            KeyCode::Char('h') => self.fs.parent_dir(),
            KeyCode::Char(' ') => {
                self.fs.select_current(current_idx);
                Ok(())
            }
            KeyCode::Char('c') => self.fs.copy_selected(true),
            KeyCode::Char('x') => self.fs.copy_selected(false),
            KeyCode::Char('v') => self.fs.paste(),
            KeyCode::Char('d') => {
                self.input_context = InputContext::ConfirmDelete;
                self.input_buffer.clear();
                self.fs.status_info = "Removed files cannot recover (y/N): ".to_string();
                self.fs.status_flag = StatusFlag::Others;
                Ok(())
            }
            KeyCode::Char('u') => self.fs.undo(),
            KeyCode::Char('n') => self.start_input(false),
            KeyCode::Char('m') => self.start_input(true),
            KeyCode::Char('r') => self.start_rename(current_idx),
            KeyCode::Char('q') => {
                self.should_quit = true;
                Ok(())
            },
            _ => Ok(()),
        };
        Ok(())
    }

    fn previous_row(&mut self) -> Result<()> {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.fs.files().len().saturating_sub(1)
                } else {
                    i.saturating_sub(1)
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        Ok(())
    }

    fn next_row(&mut self) -> Result<()> {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.fs.files().len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        Ok(())
    }

    fn start_input(&mut self, is_dir: bool) -> Result<()> {
        self.input_context = if is_dir {
            InputContext::NewDir
        } else {
            InputContext::NewFile
        };
        self.input_buffer.clear();
        self.fs.status_info = format!("Enter {} name:", if is_dir { "directory" } else { "file" });
        self.fs.status_flag = StatusFlag::Input;
        Ok(())
    }

    fn start_rename(&mut self, current_idx: usize) -> Result<()> {
        let file_name = self.fs.files().get(current_idx).map(|f| f.name.clone());
        if let Some(name) = file_name {
            self.fs.selected_index = Some(current_idx);
            self.input_buffer = name;
            self.input_context = InputContext::Rename;
            self.fs.status_info = "Enter new name:".to_string();
            self.fs.status_flag = StatusFlag::Input;
        }
        Ok(())
    }
    fn cancel_input(&mut self) {
        self.input_context = InputContext::None;
        self.input_buffer.clear();
        self.fs.status_info = "Ready".to_string();
        self.fs.status_flag = StatusFlag::Ready;
    }

    fn ui(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
            .split(f.area());

        self.render_and_display_table(f, chunks[0]);

        let status_style = match self.input_context {
            InputContext::ConfirmDelete => Style::default().fg(Color::Magenta),  // 品红色 for ConfirmDelete
            _ => match self.fs.status_flag {
                StatusFlag::Error => Style::default().fg(Color::Red),
                StatusFlag::Ready => Style::default().fg(Color::Green),
                StatusFlag::Input => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            },
        };

        if self.input_context != InputContext::None && self.input_context != InputContext::ConfirmDelete {
            let input = Paragraph::new(self.input_buffer.as_str())
                .block(Block::default().borders(Borders::ALL).title("Input"))
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(input, chunks[1]);
        } else {
            let status_text = if self.input_context == InputContext::ConfirmDelete {
                self.fs.status_info.as_str()
            } else {
                self.fs.status_info.as_str()
            };
            let status = Paragraph::new(status_text)
                .block(Block::default().borders(Borders::ALL).title(if self.input_context == InputContext::ConfirmDelete { "Confirm Delete" } else { "Status" }))
                .style(status_style);
            f.render_widget(status, chunks[1]);
        }
    }

    fn render_and_display_table(&mut self, f: &mut Frame, area: ratatui::layout::Rect) {
        let header_style = Style::default().add_modifier(Modifier::BOLD);
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let rows: Vec<Row> = self.fs.files().iter().enumerate().map(|(i, file)| {
            let type_cell = if file.is_dir { "DIR" } else { "FILE" };
            let size_str = if file.is_dir {
                "-".to_string()
            } else {
                format_size(file.size)
            };

            let style = if Some(i) == self.fs.selected_index() {
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(file.name.clone()),
                Cell::from(size_str),
                Cell::from(type_cell),
            ]).style(style)
        }).collect();

        let header = Row::new(vec![
            Cell::from("Name"),
            Cell::from("Size"),
            Cell::from("Type"),
        ]).style(header_style);

        let table = Table::new(
            rows,
            &[
                Constraint::Min(30),
                Constraint::Length(12),
                Constraint::Length(6)
            ]
        )
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(self.fs.current_dir().display().to_string())
            )
            .row_highlight_style(selected_style)
            .column_spacing(1);

        f.render_stateful_widget(table, area, &mut self.table_state);
    }
}

fn format_size(size: u64) -> String {
    if size == 0 {
        "0 B".to_string()
    } else {
        let units = ["B", "KB", "MB", "GB"];
        let mut s = size as f64;
        let mut unit_idx = 0;
        while s >= 1024.0 && unit_idx < units.len() - 1 {
            s /= 1024.0;
            unit_idx += 1;
        }
        format!("{:.1} {}", s, units[unit_idx])
    }
}