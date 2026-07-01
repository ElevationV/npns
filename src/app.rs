use std::io::Read;
use std::path::{Path, PathBuf};
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Widget;

use crate::fs::{FileEntry, FileSystemCore, DuplicatedFileHandleOps, StateFlag};
use crate::ui::input::{read_key, read_key_timeout, KeyCode};
use crate::ui::tui::Tui;
use crate::ui::widget::{
    dialog::ConflictDialog,
    file_list::FileList,
    preview::Preview,
    status::StatusBar,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(PartialEq, Clone, Copy)]
enum InputContext { 
    None, 
    NewFile, 
    NewDir, 
    Rename, 
    ConfirmDelete, 
    Search 
}

#[derive(PartialEq, Clone, Copy)]
enum ConflictKind { 
    File, 
    Dir 
}

// handle conflict
struct PasteDialog {
    path:         PathBuf,
    kind:         ConflictKind,
    apply_to_all: bool,
    cursor:       usize,
    rename_input: Option<String>,
}

impl PasteDialog {
    fn new(path: PathBuf, is_dir: bool) -> Self {
        Self { path, kind: if is_dir { ConflictKind::Dir } else { ConflictKind::File },
               apply_to_all: false, cursor: 0, rename_input: None }
    }

    fn options(&self) -> &'static [&'static str] {
        match self.kind {
            ConflictKind::File => &["Overwrite", "Rename", "Skip", "Cancel"],
            ConflictKind::Dir  => &["Write In",  "Rename", "Skip", "Cancel"],
        }
    }

    fn resolve(&self) -> DuplicatedFileHandleOps {
        match self.cursor {
            0 => match self.kind {
                ConflictKind::File => DuplicatedFileHandleOps::Overwrite,
                ConflictKind::Dir  => DuplicatedFileHandleOps::WriteIn,
            },
            1 => DuplicatedFileHandleOps::Rename(self.rename_input.clone().unwrap_or_default()),
            2 => DuplicatedFileHandleOps::Skip,
            _ => DuplicatedFileHandleOps::Cancel,
        }
    }
}

pub enum ExitAction {
    ChangeTo(String),
    Stay,
}

pub struct App {
    fs:            FileSystemCore,
    view:          Vec<FileEntry>,
    list_pos:      usize,
    scroll_offset: usize,
    marked:        Option<usize>,
    // search
    input_ctx:     InputContext,
    input_buf:     String,
    show_hidden:   bool,
    search_query:  String,
    // preview
    preview_rx:    Option<Receiver<String>>,
    preview_text:  String,
    view_dirty:    bool,
    // quit option
    should_quit:   bool,
    stay_on_quit: bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<Self> {
        let mut app = Self {
            fs:            FileSystemCore::init(start_dir),
            view:          Vec::new(),
            list_pos:      0,
            scroll_offset: 0,
            marked:        None,
            // search
            input_ctx:     InputContext::None,
            input_buf:     String::new(),
            show_hidden:   false,
            search_query:  String::new(),
            // preview
            preview_rx:    None,
            preview_text:  String::new(),
            view_dirty:    false,
            // quit option
            should_quit:   false,
            stay_on_quit:  true
        };
        app.rebuild_view();
        app.spawn_preview();
        Ok(app)
    }

    pub fn run(&mut self, tui: &mut Tui) -> Result<ExitAction> {
        let mut needs_redraw = true;
        loop {
            if self.poll_preview() { needs_redraw = true; }

            if needs_redraw {
                tui.draw(|frame| self.render(frame.area(), frame.buffer_mut()))?;
                needs_redraw = false;
            }

            if self.should_quit { break; }

            // Block up to 500 ms; only redraw when something actually changed.
            let Some(key) = read_key_timeout(500)? else { continue };
            needs_redraw = true;

            let prev_pos = self.list_pos;

            if key == KeyCode::Char('v') && self.input_ctx == InputContext::None {
                self.run_paste(tui)?;
            } else {
                self.handle_key(key);
            }

            if self.list_pos != prev_pos || self.view_dirty {
                self.preview_text.clear();
                self.preview_rx = None;
                self.spawn_preview();
            }

            if self.view_dirty {
                self.scroll_offset = 0;
                self.view_dirty = false;
            } else {
                self.clamp_scroll(tui.size()?.height as usize);
            }
        }
        if self.stay_on_quit {
            Ok(ExitAction::Stay)
        } else {
            let path = self.fs.current_dir().to_string_lossy().into_owned();
            Ok(ExitAction::ChangeTo(path))
        }
    }

    fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let [main, status_area] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
        ]).areas(area);

        let [left, right] = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ]).areas(main);

        let title = if self.search_query.is_empty() {
            self.fs.current_dir().display().to_string()
        } else {
            format!("{} [/{}]", self.fs.current_dir().display(), self.search_query)
        };

        let marked_view = self.marked.and_then(|orig| {
            self.fs.get_file(orig)
                .and_then(|p| self.view.iter().position(|e| &e.path == p))
        });

        FileList {
            entries:       &self.view,
            cursor:        self.list_pos,
            scroll_offset: self.scroll_offset,
            marked:        marked_view,
            title:         &title,
        }.render(left, buf);

        let preview_content = self.build_preview_content();
        let preview_title = self.view.get(self.list_pos)
            .and_then(|e| e.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Preview".to_string());

        Preview {
            title:   &preview_title,
            content: &preview_content,
        }.render(right, buf);

        let status  = self.build_status();
        let info    = self.build_info();
        StatusBar { status: &status, info: &info }.render(status_area, buf);
    }

    fn run_paste(&mut self, tui: &mut Tui) -> Result<()> {
        // paste() calls the closure synchronously and never stores it, so splitting the
        // borrow via raw pointers is sound: both pointers remain valid for the entire call.
        let self_ptr = self as *mut App;
        let tui_ptr  = tui  as *mut Tui;

        self.fs.paste(move |path, is_dir| -> (DuplicatedFileHandleOps, bool) {
            let app = unsafe { &mut *self_ptr };
            let tui = unsafe { &mut *tui_ptr };
            let mut dialog = PasteDialog::new(path.clone(), is_dir);

            loop {
                let filename = dialog.path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();

                let _ = tui.draw(|frame| {
                    let area = frame.area();
                    let buf  = frame.buffer_mut();
                    app.render(area, buf);
                    ConflictDialog {
                        filename:     &filename,
                        options:      dialog.options(),
                        cursor:       dialog.cursor,
                        apply_to_all: dialog.apply_to_all,
                        rename_input: dialog.rename_input.as_deref(),
                    }.render(area, buf);
                });

                let key = match read_key() {
                    Ok(k)  => k,
                    Err(_) => return (DuplicatedFileHandleOps::Cancel, true),
                };

                if dialog.rename_input.is_some() {
                    match key {
                        KeyCode::Char(c)   => { dialog.rename_input.as_mut().unwrap().push(c); }
                        KeyCode::Backspace => { dialog.rename_input.as_mut().unwrap().pop(); }
                        KeyCode::Enter     => return (dialog.resolve(), dialog.apply_to_all),
                        KeyCode::Esc       => { dialog.rename_input = None; }
                        _ => {}
                    }
                    continue;
                }

                match key {
                    KeyCode::Char('j') | KeyCode::Down
                        if dialog.cursor + 1 < dialog.options().len() => { dialog.cursor += 1; }
                    KeyCode::Char('k') | KeyCode::Up
                        if dialog.cursor > 0 => { dialog.cursor -= 1; }
                    KeyCode::Char('a') | KeyCode::Char(' ') => {
                        dialog.apply_to_all = !dialog.apply_to_all;
                    }
                    KeyCode::Enter => {
                        if dialog.cursor == 1 {
                            dialog.rename_input = Some(
                                path.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            );
                        } else {
                            return (dialog.resolve(), dialog.apply_to_all);
                        }
                    }
                    KeyCode::Esc => return (DuplicatedFileHandleOps::Cancel, true),
                    _ => {}
                }
            }
        });

        self.reset_view();
        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) {
        if self.input_ctx != InputContext::None {
            match key {
                KeyCode::Char(c)   => { self.input_buf.push(c); }
                KeyCode::Backspace => { self.input_buf.pop(); }
                KeyCode::Enter     => self.submit_input(),
                KeyCode::Esc       => self.exit_input(),
                _ => {}
            }
        } else {
            self.handle_normal(key);
        }
    }

    fn handle_normal(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('j') | KeyCode::Down  => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up    => self.move_cursor(-1),
            KeyCode::Char('h') | KeyCode::Left  => self.go_parent(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.enter_dir(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('c') => self.copy_marked(true),
            KeyCode::Char('x') => self.copy_marked(false),
            KeyCode::Char('u') => { self.fs.undo(); }
            KeyCode::Char('r') => self.start_rename(),
            KeyCode::Char('d') if self.marked.is_some() => {
                self.input_ctx = InputContext::ConfirmDelete;
                self.input_buf.clear();
            }
            KeyCode::Char('n') => { self.input_ctx = InputContext::NewFile;  self.input_buf.clear(); }
            KeyCode::Char('m') => { self.input_ctx = InputContext::NewDir;   self.input_buf.clear(); }
            KeyCode::Char('/') => { self.input_ctx = InputContext::Search;   self.input_buf.clear(); }
            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.search_query.clear();
                self.reset_view();
            }
            KeyCode::Esc if !self.search_query.is_empty() => {
                self.search_query.clear();
                self.reset_view();
            }
            KeyCode::Char('q') => { self.should_quit = true; self.stay_on_quit = false}
            KeyCode::Char('Q') => {self.should_quit = true}
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let input = self.input_buf.trim().to_string();
        match self.input_ctx {
            InputContext::Search => {
                self.search_query = input;
                self.marked = None;
                self.reset_view();
            }
            InputContext::ConfirmDelete => {
                if input.eq_ignore_ascii_case("y") {
                    if let Some(idx) = self.marked { let _ = self.fs.select(idx); }
                    self.fs.remove_selected();
                    self.marked = None;
                    self.reset_view();
                }
            }
            InputContext::NewFile => {
                if !input.is_empty() { self.fs.new_file(&input, false); self.reset_view(); }
            }
            InputContext::NewDir => {
                if !input.is_empty() { self.fs.new_file(&input, true); self.reset_view(); }
            }
            InputContext::Rename => {
                if !input.is_empty() {
                    if let Some(idx) = self.marked { let _ = self.fs.select(idx); }
                    self.fs.rename_selected(&input);
                    self.marked = None;
                    self.reset_view();
                }
            }
            InputContext::None => {}
        }
        self.exit_input();
    }

    fn exit_input(&mut self) {
        self.input_ctx = InputContext::None;
        self.input_buf.clear();
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.view.len();
        if len == 0 { self.list_pos = 0; return; }
        self.list_pos = if delta > 0 {
            if self.list_pos + 1 >= len { 0 } else { self.list_pos + 1 }
        } else {
            if self.list_pos == 0 { len - 1 } else { self.list_pos - 1 }
        };
    }

    fn go_parent(&mut self) {
        self.fs.parent_dir();
        self.marked = None;
        self.reset_view();
    }

    fn enter_dir(&mut self) {
        let Some(entry) = self.view.get(self.list_pos) else { return };
        if !entry.is_dir { return; }
        let path = entry.path.clone();
        let orig = self.view_to_orig(&path);
        let _ = self.fs.select(orig);
        self.fs.enter_selected();
        self.search_query.clear();
        self.marked = None;
        self.reset_view();
    }

    fn toggle_mark(&mut self) {
        let Some(entry) = self.view.get(self.list_pos) else { return };
        let orig = self.view_to_orig(&entry.path);
        self.marked = if self.marked == Some(orig) { None } else { Some(orig) };
    }

    fn copy_marked(&mut self, is_copy: bool) {
        if let Some(idx) = self.marked {
            let _ = self.fs.select(idx);
            self.fs.copy_selected(is_copy);
        }
    }

    fn start_rename(&mut self) {
        let Some(orig) = self.marked else { return };
        if let Some(path) = self.fs.get_file(orig) {
            self.input_buf = path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.input_ctx = InputContext::Rename;
        }
    }

    fn rebuild_view(&mut self) {
        let query = self.search_query.to_lowercase();
        self.view = self.fs.entries().iter()
            .filter(|e| {
                let name = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (self.show_hidden || !name.starts_with('.'))
                && (query.is_empty() || name.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
    }

    fn reset_view(&mut self) {
        self.rebuild_view();
        self.list_pos  = 0;
        self.view_dirty = true;
    }

    fn view_to_orig(&self, path: &Path) -> usize {
        self.fs.entries().iter().position(|e| e.path == path).unwrap_or(0)
    }

    fn clamp_scroll(&mut self, terminal_rows: usize) {
        let visible = terminal_rows.saturating_sub(7);
        if self.list_pos < self.scroll_offset {
            self.scroll_offset = self.list_pos;
        } else if self.list_pos >= self.scroll_offset + visible {
            self.scroll_offset = self.list_pos + 1 - visible;
        }
    }

    fn spawn_preview(&mut self) {
        let Some(entry) = self.view.get(self.list_pos) else { return };
        if entry.is_dir {
            // Dir preview is cheap enough to build inline; no thread needed.
            self.preview_text = build_dir_preview(&entry.path, self.show_hidden);
            return;
        }
        let path = entry.path.clone();
        let (tx, rx) = mpsc::channel();
        self.preview_rx = Some(rx);
        thread::spawn(move || { let _ = tx.send(read_file_preview(&path)); });
    }

    // Returns true if new preview data arrived (caller should redraw).
    fn poll_preview(&mut self) -> bool {
        if let Some(rx) = &self.preview_rx && let Ok(text) = rx.try_recv() {
            self.preview_text = text;
            self.preview_rx   = None;
            return true;
        }
        false
    }

    fn build_preview_content(&self) -> String {
        let Some(entry) = self.view.get(self.list_pos) else {
            return String::new();
        };
        if entry.is_dir {
            return self.preview_text.clone();
        }
        let size = fmt_size(entry.size);
        if self.preview_text.is_empty() {
            format!("Size: {}\n\nLoading...", size)
        } else {
            format!("Size: {}\n\n{}", size, self.preview_text)
        }
    }

    fn build_status(&self) -> String {
        match self.input_ctx {
            InputContext::Search        => format!("Search: {}_",       self.input_buf),
            InputContext::ConfirmDelete => format!("Delete? (y/N): {}", self.input_buf),
            InputContext::NewFile       => format!("New file: {}_",     self.input_buf),
            InputContext::NewDir        => format!("New dir: {}_",      self.input_buf),
            InputContext::Rename        => format!("Rename to: {}_",    self.input_buf),
            InputContext::None => {
                let mut parts: Vec<String> = Vec::new();
                if self.fs.state_flag() == StateFlag::Error {
                    parts.push(format!("[ERR] {}", self.fs.state_info()));
                }
                if !self.search_query.is_empty() { parts.push(format!("/{}", self.search_query)); }
                if self.show_hidden { parts.push("hidden".to_string()); }
                if let Some(idx) = self.marked
                    && let Some(path) = self.fs.get_file(idx)
                {
                    let name = path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    parts.push(format!("marked: {}", name));
                }
                if parts.is_empty() { "Ready".to_string() } else { parts.join("  │  ") }
            }
        }
    }

    fn build_info(&self) -> String {
        let total = self.view.len();
        let pos   = if total == 0 { 0 } else { self.list_pos + 1 };
        let perms = self.view.get(self.list_pos)
            .and_then(|e| e.path.symlink_metadata().ok())
            .map(|m| fmt_permissions(m.permissions().mode()))
            .unwrap_or_else(|| "----------".to_string());
        format!("{}  {}/{}", perms, pos, total)
    }
}

fn read_file_preview(path: &Path) -> String {
    use std::io::{BufRead, BufReader};

    if path.is_symlink() {
        return std::fs::read_link(path)
            .map(|t| format!("symlink -> {}", t.display()))
            .unwrap_or_else(|_| "symlink -> ?".to_string());
    }
    if !path.symlink_metadata().map(|m| m.file_type().is_file()).unwrap_or(false) {
        return path.extension().and_then(|e| e.to_str())
            .map(|e| format!("{} file", e))
            .unwrap_or_else(|| "special file".to_string());
    }
    let file = match std::fs::File::open(path) {
        Ok(f)  => f,
        Err(_) => return String::new(),
    };
    let mut reader = BufReader::new(file.take(64 * 1024));
    let mut lines  = Vec::new();
    let mut buf    = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0)  => break,
            Ok(_)  => {
                if std::str::from_utf8(buf.as_bytes()).is_err() {
                    let ext = path.extension().and_then(|e| e.to_str())
                        .map(|e| format!("{} file", e))
                        .unwrap_or_else(|| "binary".to_string());
                    return format!("{}\n<binary>", ext);
                }
                lines.push(buf.trim_end_matches('\n').to_owned());
            }
            Err(_) => break,
        }
    }
    lines.join("\n")
}

fn build_dir_preview(path: &Path, show_hidden: bool) -> String {
    let mut entries: Vec<(String, bool)> = match std::fs::read_dir(path) {
        Err(_) => return "(permission denied)".to_string(),
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter_map(|e| {
                let name   = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !show_hidden && name.starts_with('.') { return None; }
                Some((name, is_dir))
            })
            .collect(),
    };
    entries.sort_by(|(na, da), (nb, db)| match (da, db) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _             => na.cmp(nb),
    });
    entries.iter()
        .map(|(n, d)| if *d { format!("> {}", n) } else { format!("  {}", n) })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fmt_size(bytes: u64) -> String {
    if bytes < 1_024            { format!("{} B", bytes) }
    else if bytes < 1_048_576   { format!("{:.1} KB", bytes as f64 / 1_024.0) }
    else if bytes < 1_073_741_824 { format!("{:.1} MB", bytes as f64 / 1_048_576.0) }
    else                        { format!("{:.1} GB", bytes as f64 / 1_073_741_824.0) }
}

fn fmt_permissions(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd', 0o120000 => 'l', 0o060000 => 'b',
        0o020000 => 'c', 0o010000 => 'p', 0o140000 => 's',
        _        => '-',
    };
    let mut s = String::with_capacity(10);
    s.push(kind);
    for (bit, ch) in [
        (0o400,'r'),(0o200,'w'),(0o100,'x'),
        (0o040,'r'),(0o020,'w'),(0o010,'x'),
        (0o004,'r'),(0o002,'w'),(0o001,'x'),
    ] { s.push(if mode & bit != 0 { ch } else { '-' }); }
    s
}
