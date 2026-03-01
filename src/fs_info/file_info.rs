use std::path::PathBuf;

/// # Single file information
#[derive(Debug, Clone)]
pub struct FileInfo {
    name: String,
    path: PathBuf,
    size: u64,
    is_dir: bool,
}

impl FileInfo {
    pub fn new(name: String, path: PathBuf, size: u64, is_dir: bool) -> Self {
        Self {
            name,
            path,
            size,
            is_dir,
        }
    }

    // immutable
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
}
