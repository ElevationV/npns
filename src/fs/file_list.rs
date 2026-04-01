use std::cmp::Ordering;
use std::fs::{read_dir};
use std::io;
use std::path::{PathBuf, Path};

/// Catalogue of a dir
pub struct FileList {
    files: Vec<PathBuf>,
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
            
            self.push(file_path);
        }

        self.sort();
        Ok(())
    }

    pub fn sort(&mut self) {
        self.files.sort_by(|a, b| compare_by_is_dir_then_name(a, b));
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

    pub fn get_selected_file(&self) -> Result<&PathBuf, String> {
        match self.selected_index {
            Some(index) => Ok(&self.files[index]),
            None => Err("No File Selected!".to_string()),
        }
    }

    // add items
    pub fn push(&mut self, file: PathBuf) {
        self.files.push(file);
    }

    //getters
    pub fn get(&self, index: usize) -> Option<&PathBuf> {
        self.files.get(index)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
    
    pub fn files(&self) -> &[PathBuf] {
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
fn compare_by_is_dir_then_name(a: &Path, b: &Path) -> Ordering {
    match (a.is_dir(), b.is_dir()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.file_name().cmp(&b.file_name()),
    }
}
