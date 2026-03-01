use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

// macros
use crate::{
    check_or_abort_paste,
    check_or_return,
    check_or_return_with,
};
// modules
use crate::fs_info::{
    clipboard::Clipboard,
    duplicate_handler::{DuplicatedFileHandleOps, PasteAbort, ApplyToAll, FileConflictResult, DirConflictResult},
    file_info::FileInfo,
    file_list::FileList,
    history::History,
    operations::{OperationFS, OperationUnitFS},
    state::{FileSysState, StateFlag},
};
// functions
use crate::fs_info::duplicate_handler::{handle_file_duplicate, handle_dir_duplicate};

use crate::fs_info::state::StateFlag::*;

// FileSystemCore
pub struct FileSystemCore {
    current_dir: PathBuf,
    file_list:   FileList,
    state:       FileSysState,
    clipboard:   Clipboard,
    history:     History,
}

// Public Interface
impl FileSystemCore {
    pub fn init(start_dir: PathBuf) -> Self {
        let mut fs = FileSystemCore {
            current_dir: start_dir,
            file_list:   FileList::new(),
            state:       FileSysState::new(),
            clipboard:   Clipboard::new(),
            history:     History::new(),
        };
        fs.refresh();
        fs
    }

    pub fn refresh(&mut self) {
        self.file_list.deselect();
        check_or_return!(
            self,
            "Loading Dir",
            self.file_list.load_dir(self.current_dir.clone())
        );
        self.set_state(Ready, "Ready");
    }

    pub fn new_file(&mut self, name: &str, is_dir: bool) {
        let destiny_path = self.here().join(name);
        if destiny_path.exists() {
            self.set_state(Error, format!("{name} already exists"));
            return;
        }
        if is_dir {
            check_or_return!(self, "Creating Directory", fs::create_dir(&destiny_path));
        } else {
            check_or_return!(self, "Creating File", fs::File::create(&destiny_path));
        }
        self.push_history(OperationFS::New, PathBuf::new(), destiny_path);
        self.refresh();
    }

    pub fn copy_selected(&mut self, is_copy: bool) {
        let result = self.get_selected_file_info();
        let file = check_or_return!(self, "Copy/Cut File", result);
        let path = file.path().clone();
        let name = file.name().to_string();
        self.clipboard.set(path, is_copy);
        self.set_state(
            Ready,
            format!("{}: {}", if is_copy { "Copied" } else { "Cut" }, name),
        );
    }

    pub fn paste<F>(&mut self, handle_duplicate: F)
    where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        // set state
        self.set_operating_state("Pasting");
        // source path and operation
        let (source, is_copy) =
            check_or_return!(self, "Get From Clipboard", self.get_clipboard()).clone();
        // destiny dir
        let destiny_dir = self.here();
        
        if source.is_dir() {
            self.paste_dir(&source, &destiny_dir, is_copy, &handle_duplicate);
        } else {
            self.paste_file(&source, &destiny_dir, is_copy, &handle_duplicate);
        }

        self.refresh();
    }

    pub fn get_description(&self, index: usize) -> OsString {
        match self.file_list.get(index) {
            Some(file) => {
                if file.is_dir() {
                    check_or_return_with!(
                        self,
                        self.get_dir_preview::<&PathBuf, 10>(file.path()),
                        OsString::from_str("Fail to get preview").unwrap()
                    )
                } else {
                    check_or_return_with!(
                        self,
                        self.get_file_description(file.path()),
                        OsString::from_str("Fail to get preview").unwrap()
                    )
                }
            }
            None => OsString::from_str("Nothing Here").unwrap(),
        }
    }

    pub fn rename_selected(&mut self, new_name: &str) {
        let file = check_or_return!(self, "Rename", self.get_selected_file_info());
        let old_path = file.path().clone();
        let new_path = check_or_return!(self, "Rename", rename_file_name(&old_path, new_name));
        check_or_return!(self, "Rename", fs::rename(&old_path, &new_path));
        self.push_history(OperationFS::Rename, old_path, new_path);
        self.refresh();
    }

    fn change_dir(&mut self, target: PathBuf) -> Result<(), String> {
        if !target.exists() {
            return Err("Target path does not exist".to_string());
        }
        if !target.is_dir() {
            return Err("Target path is not a directory".to_string());
        }
        let old_dir = self.current_dir.clone();
        self.current_dir = target.clone();
        self.push_history(OperationFS::ChangeDir, old_dir, target);
        self.refresh();
        Ok(())
    }

    pub fn parent_dir(&mut self) {
        let parent = match self.current_dir.parent() {
            Some(p) => p.to_path_buf(),
            None => {
                self.set_state(Error, "Already at root directory");
                return;
            }
        };
        match self.change_dir(parent) {
            Ok(()) => self.set_state(Ready, "Moved to parent directory"),
            Err(e) => self.set_state(Error, format!("Failed to go to parent: {}", e)),
        }
    }

    pub fn enter_selected(&mut self) {
        let (file_path, file_name, is_dir) = match self.get_selected_file_info() {
            Ok(f) => (f.path().clone(), f.name().to_string(), f.is_dir()),
            Err(e) => { self.set_state(Error, e); return; }
        };
        if !is_dir {
            self.set_state(Error, "Selected item is not a directory");
            return;
        }
        match self.change_dir(file_path) {
            Ok(()) => self.set_state(Ready, format!("Entered: {}", file_name)),
            Err(e) => self.set_state(Error, format!("Failed to enter directory: {}", e)),
        }
    }

    pub fn undo(&mut self) {
        if !self.history_is_available() {
            self.set_state(Error, "Undo Not Available!");
            return;
        }
        self.set_operating_state("Undo");
        let op = match check_or_return!(self, "Undo", self.pop_history()) {
            Some(o) => o,
            None => { self.set_state(Error, "Nothing to undo"); return; }
        };

        let source  = op.file_source.clone();
        let destiny = op.file_destiny.clone();

        match op.operation {
            OperationFS::Copy => {
                let r = if destiny.exists() {
                    remove_file(&destiny, destiny.is_dir())
                        .map_err(|e| format!("Failed to remove copied file: {}", e))
                } else { Ok(()) };
                check_or_return!(self, "Undo Copy", r);
            }
            OperationFS::Move => {
                let r = if destiny.exists() {
                    if let Some(p) = source.parent() { let _ = fs::create_dir_all(p); }
                    fs::rename(&destiny, &source)
                        .map_err(|e| format!("Failed to move file back: {}", e))
                } else {
                    Err("Target file doesn't exist".to_string())
                };
                check_or_return!(self, "Undo Move", r);
            }
            OperationFS::Rename => {
                let r = if destiny.exists() {
                    fs::rename(&destiny, &source)
                        .map_err(|e| format!("Failed to restore original name: {}", e))
                } else {
                    Err("Renamed file doesn't exist".to_string())
                };
                check_or_return!(self, "Undo Rename", r);
            }
            OperationFS::New => {
                let r = if destiny.exists() {
                    remove_file(&destiny, destiny.is_dir())
                        .map_err(|e| format!("Failed to remove new file: {}", e))
                } else { Ok(()) };
                check_or_return!(self, "Undo New", r);
            }
            OperationFS::ChangeDir => {
                self.current_dir = source;
                self.refresh();
            }
            OperationFS::Remove => {
                self.set_state(Error, "Cannot undo remove operation (not recoverable)");
                return;
            }
            OperationFS::EndRange => {
                while let Some(inner) = check_or_return!(self, "Undo", self.pop_history()) {
                    if inner.operation == OperationFS::StartRange { break; }
                    if self.state.flag() == Error {
                        self.push_history(inner.operation, inner.file_source, inner.file_destiny);
                        self.push_history(OperationFS::EndRange, PathBuf::new(), PathBuf::new());
                        return;
                    }
                    // Push back so undo() can pop it — undo() always pops one entry itself
                    self.push_history(inner.operation, inner.file_source, inner.file_destiny);
                    self.undo();
                }
            }
            OperationFS::StartRange => {
                self.set_state(Error, "Unexpected StartRange in undo");
                return;
            }
            OperationFS::None => return,
        }

        self.refresh();
        self.set_state(Ready, "Undone");
    }
    
    pub fn remove_selected(&mut self) { 
        todo!() 
    }

    pub fn files(&self) -> &[FileInfo] { self.file_list.files() }
    pub fn get_file(&self, index: usize) -> Option<&FileInfo> { self.file_list.get(index) }
    pub fn current_dir(&self) -> &PathBuf { &self.current_dir }
    pub fn state_flag(&self) -> StateFlag { self.state.flag() }
    pub fn state_info(&self) -> &str { self.state.info() }
    pub fn select(&mut self, index: usize) -> Result<(), String> { self.file_list.select(index) }
}

// Private helpers
impl FileSystemCore {
    fn set_state<S: Into<String>>(&mut self, flag: StateFlag, info: S) {
        self.state.set(flag, info);
    }
    fn set_operating_state<S: Into<String>>(&mut self, op: S) {
        self.state.set(Operating, op);
    }
    fn get_selected_file_info(&self) -> Result<&FileInfo, String> {
        self.file_list.get_selected_file()
    }
    fn get_clipboard(&self) -> Result<&(PathBuf, bool), String> {
        self.clipboard.get().ok_or_else(|| "Clipboard is empty".to_string())
    }
    fn remove_by_name(&mut self, name: &PathBuf) -> Result<(), io::Error> {
        let is_dir = name.is_dir();
        if self.history_is_available() {
            self.push_history(OperationFS::Remove, name.clone(), PathBuf::new());
        }
        remove_file(name, is_dir)
    }
    fn here(&self) -> PathBuf { self.current_dir.clone() }
    fn pop_history(&mut self) -> Result<Option<OperationUnitFS>, String> {
        self.history.pop().map_err(|e| format!("History pop failed: {}", e))
    }
    fn push_history(&mut self, operation: OperationFS, src: PathBuf, dst: PathBuf) {
        if let Err(e) = self.history.push(operation, src, dst) {
            self.set_state(Error, format!("History record failed: {}", e));
        }
    }
    fn history_is_available(&self) -> bool { self.history.is_available() }
}


// Paste helpers
impl FileSystemCore {
    // Paste a single file.  Calls handle_file_duplicate for conflict resolution
    fn paste_file<F>(
        &mut self,
        source:      &PathBuf,
        destiny_dir: &Path,
        is_copy:     bool,
        callback:    &F,
    ) where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        // get source file name
        let file_name = check_or_return!(
            self, "Get File Name",
            source.file_name().ok_or("source path has no file name")
        );
        // construct 'initial' destiny path(will handle duplicate later)
        let initial = destiny_dir.join(file_name);
        // resolve file conflicts
        let (final_dest, handler) = match handle_file_duplicate(initial, callback) {
            FileConflictResult::Cancel => {
                self.set_state(Ready, "Cancelled");
                return;
            }
            FileConflictResult::Skip { .. } => {
                self.set_state(Ready, "Skipped");
                return;
            }
            FileConflictResult::Proceed { dest, handler, .. } => (dest, handler),
        };

        // For Overwrite: delete the existing file first (recorded in history)
        if matches!(handler, DuplicatedFileHandleOps::Overwrite) {
            check_or_return!(self, "Overwrite File", self.remove_by_name(&final_dest));
        }

        let operation = if is_copy {
            check_or_return!(self, "Copy File", fs::copy(source, &final_dest));
            OperationFS::Copy
        } else {
            check_or_return!(self, "Move File", fs::rename(source, &final_dest));
            OperationFS::Move
        };

        self.push_history(operation, source.clone(), final_dest);
        self.set_state(Ready, "Done");
    }

    /// # Paste a directory
    ///
    /// Handles:
    ///   - Top-level conflict resolution (one call to handle_dir_duplicate)
    ///   - History StartRange / EndRange wrapping
    ///   - Rollback on failure
    ///   - Source removal on successful move
    ///
    /// Does NOT perform single file operations
    fn paste_dir<F>(
        &mut self,
        source:      &PathBuf,
        destiny_dir: &Path,
        is_copy:     bool,
        callback:    &F,
    ) where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        let dir_name = match source.file_name() {
            Some(n) => n,
            None => { self.set_state(Error, "Source directory has no name"); return; }
        };

        let initial = destiny_dir.join(dir_name);
        let destiny_root = match handle_dir_duplicate(initial, callback) {
            DirConflictResult::Cancel => {
                self.set_state(Ready, "Cancelled");
                return;
            }
            DirConflictResult::Skip { .. } => {
                self.set_state(Ready, "Skipped");
                return;
            }
            DirConflictResult::Proceed { dest, .. } => dest,
        };

        if let Err(e) = fs::create_dir_all(&destiny_root) {
            self.set_state(Error, format!("Create DIR: {e}"));
            return;
        }

        if self.history_is_available() {
            self.push_history(OperationFS::StartRange, PathBuf::new(), PathBuf::new());
            if is_copy {
                self.push_history(OperationFS::Copy, source.clone(), destiny_root.clone());
            } else {
                self.push_history(OperationFS::Move, source.clone(), destiny_root.clone());
            }
        }

        let result = self.pasting_dir(source, &destiny_root, is_copy, callback);

        if self.history_is_available() {
            self.push_history(OperationFS::EndRange, PathBuf::new(), PathBuf::new());
        }

        match result {
            Err(_) => { self.undo(); }
            Ok(()) if !is_copy => {
                if let Err(e) = fs::remove_dir(source) {
                    self.set_state(Error, format!("Remove source DIR: {e}"));
                    self.undo();
                }
            }
            Ok(()) => {}
        }
    }

    /// # Recursive copy/move of directory contents
    ///
    /// THE ONLY FUNCTION that calls the callback (via `handle_file_duplicate` and `handle_dir_duplicate`)
    ///
    /// Maintains ApplyToAll across the loop.  
    /// ApplyToAll is local to this call frame; 
    /// sub-directories get their own frame and therefore their own independent ApplyToAll state, 
    /// which is correct: apply-to-all means "all items at this level" not "all items in the entire tree"
    fn pasting_dir<F>(
        &mut self,
        source:   &PathBuf,
        destiny:  &Path,
        is_copy:  bool,
        callback: &F,
    ) -> Result<(), PasteAbort>
    where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        let mut apply_to_all = ApplyToAll::No;

        for entry in fs::read_dir(source).map_err(|_| PasteAbort::Error)? {
            let entry     = check_or_abort_paste!(self, "Reading Dir Entry",  entry);
            let file_type = check_or_abort_paste!(self, "Reading Entry Type", entry.file_type());
            let is_dir    = file_type.is_dir();

            let source_file  = entry.path();
            let destiny_file = destiny.join(entry.file_name());

            if is_dir {
                self.process_dir_entry(
                    &source_file, destiny_file, is_copy,
                    &mut apply_to_all, callback,
                )?;
            } else {
                self.process_file_entry(
                    &source_file, destiny_file, is_copy,
                    &mut apply_to_all, callback,
                )?;
            }
        }

        Ok(())
    }

    /// Process one file entry inside pasting_dir.
    fn process_file_entry<F>(
        &mut self,
        source:       &PathBuf,
        destiny:      PathBuf,
        is_copy:      bool,
        apply_to_all: &mut ApplyToAll,
        callback:     &F,
    ) -> Result<(), PasteAbort>
    where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        // Determine (final destination, handler).
        let (final_dest, handler, apply) = if !destiny.exists() {
            // No conflict — proceed directly.
            (destiny, DuplicatedFileHandleOps::None, false)
        } else if let Some(stored) = apply_to_all.get(false) {
            // apply-to-all active for files — use stored handler.
            (destiny, stored.clone(), true)
        } else {
            // Ask the user.
            match handle_file_duplicate(destiny, callback) {
                FileConflictResult::Cancel => return Err(PasteAbort::Cancel),
                FileConflictResult::Skip { apply } => {
                    apply_to_all.update(&DuplicatedFileHandleOps::Skip, apply, false);
                    return Ok(());
                }
                FileConflictResult::Proceed { dest, handler, apply } => (dest, handler, apply),
            }
        };

        // Update apply_to_all from this interaction.
        apply_to_all.update(&handler, apply, false);

        // For Overwrite: remove the existing target first.
        if matches!(handler, DuplicatedFileHandleOps::Overwrite) {
            check_or_abort_paste!(self, "Overwrite File", self.remove_by_name(&final_dest));
        }

        // Execute copy or move.
        if is_copy {
            check_or_abort_paste!(self, "Copy File", fs::copy(source, &final_dest));
            // Record copy so undo can delete the destination file.
            if self.history_is_available() {
                self.push_history(OperationFS::Copy, source.clone(), final_dest.clone());
            }
        } else {
            check_or_abort_paste!(self, "Move File", fs::rename(source, &final_dest));
        }

        Ok(())
    }

    // Process one directory entry inside pasting_dir.
    fn process_dir_entry<F>(
        &mut self,
        source:       &PathBuf,
        destiny:      PathBuf,
        is_copy:      bool,
        apply_to_all: &mut ApplyToAll,
        callback:     &F,
    ) -> Result<(), PasteAbort>
    where
        F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
    {
        // Determine (final destination, handler).
        let (final_dest, handler, apply) = if !destiny.exists() {
            (destiny, DuplicatedFileHandleOps::None, false)
        } else if let Some(stored) = apply_to_all.get(true) {
            (destiny, stored.clone(), true)
        } else {
            match handle_dir_duplicate(destiny, callback) {
                DirConflictResult::Cancel => return Err(PasteAbort::Cancel),
                DirConflictResult::Skip { apply } => {
                    apply_to_all.update(&DuplicatedFileHandleOps::Skip, apply, true);
                    return Ok(());
                }
                DirConflictResult::Proceed { dest, handler, apply } => (dest, handler, apply),
            }
        };

        // Update apply_to_all from this interaction.
        apply_to_all.update(&handler, apply, true);

        // Create the destination directory.
        check_or_abort_paste!(self, "Create DIR", fs::create_dir_all(&final_dest));

        // For copy: record New so undo can remove the created directory.
        // For move: do NOT record — the top-level Move(src_root, dst_root)
        // handles the entire tree rename on undo.
        if is_copy && self.history_is_available() {
            self.push_history(OperationFS::New, final_dest.clone(), PathBuf::new());
        }

        // Recurse — sub-directory gets its own ApplyToAll frame.
        self.pasting_dir(source, &final_dest, is_copy, callback)?;

        if !is_copy {
            check_or_abort_paste!(
                self, "Remove Source DIR",
                fs::remove_dir(source)
            );
        }

        Ok(())
    }

    fn get_dir_preview<P, const CAP: u8>(&self, path: P) -> Result<OsString, io::Error>
    where P: AsRef<Path>,
    {
        let mut preview = OsString::new();
        let mut counter = 0u8;
        for entry in fs::read_dir(path)? {
            match entry {
                Ok(e) => {
                    if counter >= CAP { break; }
                    preview.push(e.file_name());
                    preview.push("\n");
                    counter += 1;
                }
                Err(_) => continue,
            }
        }
        Ok(preview)
    }

    fn get_file_description<P>(&self, path: P) -> Result<OsString, io::Error>
    where P: AsRef<Path>,
    {
        let path = path.as_ref();
        let metadata = fs::metadata(path)?;
        let description = if metadata.is_dir() {
            "directory".to_string()
        } else if metadata.is_symlink() {
            match fs::read_link(path) {
                Ok(target) => format!("symbolic link -> {}", target.display()),
                Err(_)     => "symbolic link".to_string(),
            }
        } else {
            match infer::get_from_path(path)? {
                Some(kind) => kind.mime_type().to_string(),
                None => path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!("{} file", e))
                    .unwrap_or_else(|| "data".to_string()),
            }
        };
        Ok(OsString::from(description))
    }
}


// free functions

fn remove_file<P: AsRef<Path>>(source: P, is_dir: bool) -> Result<(), io::Error> {
    if is_dir { fs::remove_dir_all(source) } else { fs::remove_file(source) }
}

fn rename_file_name(original: &Path, name: &str) -> io::Result<PathBuf> {
    let parent = original.parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    Ok(parent.join(name))
}

