#![allow(dead_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crate::fs::{FileSystemCore, DuplicatedFileHandleOps, StateFlag};
use crate::ui::{read_key, KeyCode, Rect, Screen};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(PartialEq, Clone, Copy)]
enum InputContext {
    None,
    NewFile,
    NewDir,
    Rename,
    ConfirmDelete,
    Search,
}

#[derive(PartialEq, Clone, Copy)]
enum ConflictKind { File, Dir }

struct ConflictDialog {
    path: PathBuf,
    kind: ConflictKind,
    apply_to_all: bool,
    cursor: usize,
    rename_input: Option<String>,
}

impl ConflictDialog {
    fn new(path: PathBuf, is_dir: bool) -> Self {
        Self {
            path,
            kind: if is_dir { ConflictKind::Dir } else { ConflictKind::File },
            apply_to_all: false,
            cursor: 0,
            rename_input: None,
        }
    }

    fn options(&self) -> &'static [&'static str] {
        match self.kind {
            ConflictKind::File => &["Overwrite", "Rename", "Skip", "Cancel"],
            ConflictKind::Dir => &["Write In",  "Rename", "Skip", "Cancel"],
        }
    }

    fn to_handler(&self) -> DuplicatedFileHandleOps {
        match self.cursor {
            0 => match self.kind {
                ConflictKind::File => DuplicatedFileHandleOps::Overwrite,
                ConflictKind::Dir => DuplicatedFileHandleOps::WriteIn,
            },
            1 => DuplicatedFileHandleOps::Rename(
                self.rename_input.clone().unwrap_or_default()
            ),
            2 => DuplicatedFileHandleOps::Skip,
            _ => DuplicatedFileHandleOps::Cancel,
        }
    }
}

struct PreviewWorker {
    request_tx: Sender<Option<PathBuf>>,
    result_rx:  Receiver<String>,
    cached:     String,
}

impl PreviewWorker {
    fn spawn() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<Option<PathBuf>>();
        let (result_tx,  result_rx)  = mpsc::channel::<String>();
        thread::spawn(move || run_preview_worker(request_rx, result_tx));
        Self { request_tx, result_rx, cached: String::new() }
    }

    fn request(&self, path: Option<&Path>) {
        let _ = self.request_tx.send(path.map(PathBuf::from));
    }

    fn poll(&mut self) {
        loop {
            match self.result_rx.try_recv() {
                Ok(text)                        => { self.cached = text; }
                Err(TryRecvError::Empty)        => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn invalidate(&mut self) { self.cached.clear(); }
    fn content(&self) -> &str { &self.cached }
}

pub struct App {
    fs:            FileSystemCore,
    list_pos:      usize,
    marked:        Option<usize>,
    input_context: InputContext,
    input_buffer:  String,
    show_hidden:   bool,
    search_query:  String,
    should_quit:   bool,
    preview:       PreviewWorker,
    view:          Vec<PathBuf>,
    view_changed:  bool,
}

impl App {
    pub fn new(start_dir: PathBuf) -> Result<Self> {
        let mut app = Self {
            fs: FileSystemCore::init(start_dir),
            list_pos: 0,
            marked: None,
            input_context: InputContext::None,
            input_buffer: String::new(),
            show_hidden: false,
            search_query: String::new(),
            should_quit: false,
            preview: PreviewWorker::spawn(),
            view: Vec::new(),
            view_changed: false
        };
        app.rebuild_view();
        app.preview.request(app.cursor_path());
        Ok(app)
    }

    pub fn run(&mut self, screen: &mut Screen) -> Result<()> {
        let mut scroll_offset: usize = 0;
    
        loop {
            self.preview.poll();
            self.draw(screen, scroll_offset);
            if self.should_quit { break; }
    
            let key = read_key()?;
            if key == KeyCode::Char('v') && self.input_context == InputContext::None {
                self.paste(screen);
            } else {
                self.handle_key(key);
            }
    
            if self.view_changed {
                scroll_offset = 0;
                self.view_changed = false;
            } else {
                let visible_rows = screen.rows.saturating_sub(7) as usize;
                if self.list_pos < scroll_offset {
                    scroll_offset = self.list_pos;
                } else if self.list_pos >= scroll_offset + visible_rows {
                    scroll_offset = self.list_pos + 1 - visible_rows;
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) {
        if self.input_context != InputContext::None {
            self.handle_input_mode(key);
        } else {
            self.handle_normal_mode(key);
        }
    }

    fn handle_input_mode(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char(c) => { self.input_buffer.push(c); }
            KeyCode::Backspace => { self.input_buffer.pop(); }
            KeyCode::Enter => self.submit_input(),
            KeyCode::Esc => self.exit_input_mode(),
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let input = self.input_buffer.trim().to_string();
        match self.input_context {
            InputContext::Search => {
                self.search_query = input;
                self.reset_view();
                self.marked = None;
                self.exit_input_mode();
            }
            InputContext::ConfirmDelete => {
                if input.eq_ignore_ascii_case("y") {
                    if let Some(idx) = self.marked { let _ = self.fs.select(idx); }
                    self.fs.remove_selected();
                    self.marked = None;
                    self.reset_view();
                }
                self.exit_input_mode();
            }
            InputContext::NewFile => {
                if !input.is_empty() { self.fs.new_file(&input, false); self.reset_view(); }
                self.exit_input_mode();
            }
            InputContext::NewDir => {
                if !input.is_empty() { self.fs.new_file(&input, true); self.reset_view(); }
                self.exit_input_mode();
            }
            InputContext::Rename => {
                if !input.is_empty() {
                    if let Some(idx) = self.marked { let _ = self.fs.select(idx); }
                    self.fs.rename_selected(&input);
                    self.marked = None;
                    self.reset_view();
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

    fn handle_normal_mode(&mut self, key: KeyCode) {
        let prev_pos = self.list_pos;
        match key {
            KeyCode::Char('j')                  => self.move_cursor(1),
            KeyCode::Char('k')                  => self.move_cursor(-1),
            KeyCode::Char('h')                  => self.go_parent_dir(),
            KeyCode::Char('l') | KeyCode::Enter => self.enter_dir(),

            #[cfg(debug_assertions)]
            KeyCode::Down  => self.move_cursor(1),
            #[cfg(debug_assertions)]
            KeyCode::Up    => self.move_cursor(-1),
            #[cfg(debug_assertions)]
            KeyCode::Left  => self.go_parent_dir(),
            #[cfg(debug_assertions)]
            KeyCode::Right => self.enter_dir(),
            
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('c') => self.copy_marked(true),
            KeyCode::Char('x') => self.copy_marked(false),
            KeyCode::Char('u') => self.fs.undo(),
            KeyCode::Char('r') => self.start_rename(),
            KeyCode::Char('d') => {
                if self.marked.is_some() {
                    self.input_context = InputContext::ConfirmDelete;
                    self.input_buffer.clear();
                }
            }
            KeyCode::Char('n') => {
                self.input_context = InputContext::NewFile;
                self.input_buffer.clear();
            }
            KeyCode::Char('m') => {
                self.input_context = InputContext::NewDir;
                self.input_buffer.clear();
            }
            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.search_query.clear();
                self.reset_view();
            }
            KeyCode::Char('/') => {
                self.input_context = InputContext::Search;
                self.input_buffer.clear();
            }
            KeyCode::Esc => {
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    self.reset_view();
                }
            }
            KeyCode::Char('q') => { self.should_quit = true; }
            _ => {}
        }

        if self.list_pos != prev_pos {
            self.preview.invalidate();
            self.preview.request(self.cursor_path());
        }
    }
}

// File System Operations
impl App {
    fn move_cursor(&mut self, delta: i32) {
        let len = self.view.len();
        if len == 0 { self.list_pos = 0; return; }
        if delta > 0 {
            self.list_pos = if self.list_pos + 1 >= len { 0 } else { self.list_pos + 1 };
        } else {
            self.list_pos = if self.list_pos == 0 { len - 1 } else { self.list_pos - 1 };
        }
    }

    fn go_parent_dir(&mut self) {
        self.fs.parent_dir();
        self.marked = None;
        self.reset_view();
        self.preview.invalidate();
        self.preview.request(self.cursor_path());
    }

    fn enter_dir(&mut self) {
        let Some(path) = self.view.get(self.list_pos) else { return };
        if !path.is_dir() { return; }
        let orig_idx = self.view_to_orig_idx(self.list_pos);
        let _ = self.fs.select(orig_idx);
        self.fs.enter_selected();
        self.search_query.clear();
        self.marked = None;
        self.reset_view();
        self.preview.invalidate();
        self.preview.request(self.cursor_path());
    }

    fn toggle_mark(&mut self) {
        let orig = self.view_to_orig_idx(self.list_pos);
        self.marked = if self.marked == Some(orig) { None } else { Some(orig) };
    }

    fn copy_marked(&mut self, is_copy: bool) {
        if let Some(idx) = self.marked {
            let _ = self.fs.select(idx);
            self.fs.copy_selected(is_copy);
        }
    }

    fn start_rename(&mut self) {
        let Some(orig_idx) = self.marked else { return };
        if let Some(path) = self.fs.get_file(orig_idx) {
            self.input_buffer = path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.input_context = InputContext::Rename;
        }
    }

    fn paste(&mut self, screen: &mut Screen) {
        let screen_ptr = screen as *mut Screen;
        let self_ptr   = self   as *mut App;

        self.fs.paste(move |path, is_dir| {
            let screen = unsafe { &mut *screen_ptr };
            let app    = unsafe { &mut *self_ptr };

            let mut dialog = ConflictDialog::new(path.clone(), is_dir);

            loop {
                app.draw(screen, 0);
                let filename = dialog.path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                screen.render_conflict_dialog(
                    &filename,
                    dialog.options(),
                    dialog.cursor,
                    dialog.apply_to_all,
                    dialog.rename_input.as_deref(),
                );
                screen.present();

                let key = match read_key() {
                    Ok(k)  => k,
                    Err(_) => return (DuplicatedFileHandleOps::Cancel, true),
                };

                if dialog.rename_input.is_some() {
                    match key {
                        KeyCode::Char(c) => { dialog.rename_input.as_mut().unwrap().push(c); }
                        KeyCode::Backspace => { dialog.rename_input.as_mut().unwrap().pop(); }
                        KeyCode::Enter => return (dialog.to_handler(), dialog.apply_to_all),
                        KeyCode::Esc => { dialog.rename_input = None; }
                        _ => {}
                    }
                    continue;
                }

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
                            dialog.rename_input = Some(
                                path.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default()
                            );
                        } else {
                            return (dialog.to_handler(), dialog.apply_to_all);
                        }
                    }
                    KeyCode::Esc => return (DuplicatedFileHandleOps::Cancel, true),
                    _ => {}
                }
            }
        });

        self.reset_view();
    }

    fn rebuild_view(&mut self) {
        let query = self.search_query.to_lowercase();
        self.view = self.fs.files()
            .iter()
            .filter(|path| {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let visible = self.show_hidden || !name.starts_with('.');
                let matched = query.is_empty() || name.to_lowercase().contains(&query);
                visible && matched
            })
            .cloned()
            .collect();
    }

    fn reset_view(&mut self) {
        self.rebuild_view();
        self.list_pos = 0;
        self.view_changed = true;
    }

    fn view_to_orig_idx(&self, view_idx: usize) -> usize {
        let Some(path) = self.view.get(view_idx) else { return 0 };
        self.fs.files().iter().position(|p| p == path).unwrap_or(0)
    }

    fn cursor_path(&self) -> Option<&Path> {
        self.view.get(self.list_pos)
            .filter(|path| !path.is_dir())
            .map(|path| path.as_path())
    }
}

// Draw UI
impl App {
    fn draw(&mut self, screen: &mut Screen, scroll_offset: usize) {
        screen.resize();
        screen.clear_all();
        let pane_height = screen.rows.saturating_sub(3);
        let left_width  = screen.cols / 2;
        let right_width = screen.cols - left_width;
        let status_row  = screen.rows.saturating_sub(2);
        self.draw_file_list(screen, Rect::new(1, 1, left_width, pane_height), scroll_offset);
        self.draw_preview(screen, Rect::new(left_width + 1, 1, right_width, pane_height));
        self.draw_status_bar(screen, Rect::new(1, status_row, screen.cols, 3));
        screen.present();
    }

    fn draw_file_list(&self, screen: &mut Screen, area: Rect, scroll_offset: usize) {
        let mut title = self.fs.current_dir().display().to_string();
        if !self.search_query.is_empty() {
            title = format!("{} [/{}]", title, self.search_query);
        }

        let marked_view_pos = self.marked.and_then(|orig| {
            self.fs.get_file(orig)
                .and_then(|path| self.view.iter().position(|p| p == path))
        });

        let view_refs: Vec<&Path> = self.view.iter().map(PathBuf::as_path).collect();
        screen.render_file_list(area, &view_refs, self.list_pos, scroll_offset, marked_view_pos, &title);
    }

    fn draw_preview(&self, screen: &mut Screen, area: Rect) {
        let Some(path) = self.view.get(self.list_pos) else {
            screen.render_preview(area, "Preview", "(empty)");
            return;
        };

        let title = path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if path.is_dir() {
            let content = build_dir_preview(path, self.show_hidden);
            screen.render_preview(area, &title, &content);
        } else {
            let size = path.symlink_metadata()
                .map(|m| format_size(m.len()))
                .unwrap_or_else(|_| "?".to_string());
            let content = if self.preview.content().is_empty() {
                format!("Size: {}\n\nLoading...", size)
            } else {
                format!("Size: {}\n\n{}", size, self.preview.content())
            };
            screen.render_preview(area, &title, &content);
        }
    }

    fn draw_status_bar(&self, screen: &mut Screen, area: Rect) {
        let total = self.view.len();
        let pos   = if total == 0 { 0 } else { self.list_pos + 1 };

        let status = match self.input_context {
            InputContext::Search        => format!("Search: {}_",       self.input_buffer),
            InputContext::ConfirmDelete => format!("Delete? (y/N): {}", self.input_buffer),
            InputContext::NewFile       => format!("New file: {}_",     self.input_buffer),
            InputContext::NewDir        => format!("New dir: {}_",      self.input_buffer),
            InputContext::Rename        => format!("Rename to: {}_",    self.input_buffer),
            InputContext::None          => {
                let mut parts: Vec<String> = Vec::new();

                // Only surface errors; Ready and Operating are noise in the status bar.
                if self.fs.state_flag() == StateFlag::Error {
                    parts.push(format!("[ERR] {}", self.fs.state_info()));
                }
                if !self.search_query.is_empty() {
                    parts.push(format!("/{}", self.search_query));
                }
                if self.show_hidden {
                    parts.push("hidden".to_string());
                }
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
        };

        let permissions = self.view.get(self.list_pos)
            .and_then(|path| path.symlink_metadata().ok())
            .map(|m| format_permissions(m.permissions().mode()))
            .unwrap_or_else(|| "----------".to_string());

        let info = format!("{}  {}/{}", permissions, pos, total);

        screen.render_status_bar(area, &status, &info);
    }
}

fn run_preview_worker(requests: Receiver<Option<PathBuf>>, results: Sender<String>) {
    loop {
        let path = match recv_latest(&requests) {
            Some(path) => path,
            None       => return,
        };
        let text = match path {
            None       => String::new(),
            Some(path) => read_file_preview(&path),
        };
        if results.send(text).is_err() { return; }
    }
}

fn recv_latest(rx: &Receiver<Option<PathBuf>>) -> Option<Option<PathBuf>> {
    let first = rx.recv().ok()?;
    let mut latest = first;
    loop {
        match rx.try_recv() {
            Ok(newer)                       => { latest = newer; }
            Err(TryRecvError::Empty)        => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }
    Some(latest)
}

fn read_file_preview(path: &Path) -> String {
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::OpenOptionsExt;

    if path.is_symlink() {
        return std::fs::read_link(path)
            .map(|target| format!("symlink → {}", target.display()))
            .unwrap_or_else(|_| "symlink → ?".to_string());
    }

    if !path.symlink_metadata().map(|m| m.file_type().is_file()).unwrap_or(false) {
        return path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{} file", ext))
            .unwrap_or_else(|| "special file".to_string());
    }

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(_)   => return String::new(),
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
                    let ext = path.extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| format!("{} file", ext))
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
        Err(_)  => return "(permission denied)".to_string(),
        Ok(read_dir) => read_dir
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name  = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !show_hidden && name.starts_with('.') { return None; }
                Some((name, is_dir))
            })
            .collect(),
    };

    entries.sort_by(|(name_a, is_dir_a), (name_b, is_dir_b)| {
        match (is_dir_a, is_dir_b) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => name_a.cmp(name_b),
        }
    });

    entries.iter()
        .map(|(name, is_dir)| {
            if *is_dir { format!("> {}", name) } else { format!("  {}", name) }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_size(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{} B", bytes)
    } else if bytes < 1_024 * 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else if bytes < 1_024 * 1_024 * 1_024 {
        format!("{:.1} MB", bytes as f64 / (1_024.0 * 1_024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1_024.0 * 1_024.0 * 1_024.0))
    }
}

fn format_permissions(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd', 0o120000 => 'l', 0o060000 => 'b',
        0o020000 => 'c', 0o010000 => 'p', 0o140000 => 's',
        _        => '-',
    };
    let bits = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    let mut result = String::with_capacity(10);
    result.push(kind);
    for (bit, ch) in bits { result.push(if mode & bit != 0 { ch } else { '-' }); }
    result
}