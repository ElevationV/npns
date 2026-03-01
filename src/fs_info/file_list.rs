use crate::fs_info::file_info::FileInfo;
use std::cmp::Ordering;
use std::fs::{read_dir};
use std::io;
use std::path::PathBuf;

/// Catalogue of a dir
pub struct FileList {
    files: Vec<FileInfo>,
    selected_index: Option<usize>,
}

// public functions
impl FileList {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected_index: None,
        }
    }
    
    pub fn load_dir(&mut self, path: PathBuf) -> io::Result<()> {
        self.clear();

        for entry in read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();

            if let Some(file_name) = file_path.file_name() {
                let file_name = file_name.to_string_lossy().into_owned();

                // skip files fail to read metadata
                if let Ok(m) = file_path.metadata() {
                    self.push(FileInfo::new(file_name, file_path, m.len(), m.is_dir()));
                }
            }
        }

        self.sort();
        Ok(())
    }

    pub fn sort(&mut self) {
        self.files.sort_by(compare_by_is_dir_then_name);
    }

    // selection
    pub fn select(&mut self, index: usize) -> Result<(), String> {
        if index >= self.files.len() {
            return Err(format!(
                "SELECTING: index out of bounds {} >= {}",
                index,
                self.len()
            ));
        }
        self.selected_index = Some(index);
        Ok(())
    }

    pub fn deselect(&mut self) {
        self.selected_index = None;
    }

    pub fn get_selected_file(&self) -> Result<&FileInfo, String> {
        match self.selected_index {
            Some(index) => Ok(&self.files[index]),
            None => Err("No File Selected!".to_string()),
        }
    }


    
    // add items
    pub fn push(&mut self, file: FileInfo) {
        self.files.push(file);
    }



    //getters
    pub fn get(&self, index: usize) -> Option<&FileInfo> {
        self.files.get(index)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
    
    pub fn files(&self) -> &[FileInfo] {
        &self.files
    }
}

// private functions
impl FileList {
    fn clear(&mut self) {
        self.files.clear();
        self.selected_index = None;
    }
}

// ordering rules: dir comes first, then sorted by name
fn compare_by_is_dir_then_name(a: &FileInfo, b: &FileInfo) -> Ordering {
    match (a.is_dir(), b.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name().cmp(b.name()),
    }
}
