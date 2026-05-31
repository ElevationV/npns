use std::cmp::Ordering;
use std::fs::read_dir;
use std::io;
use std::path::PathBuf;

#[derive(Clone)]
pub struct FileEntry {
    pub path:    PathBuf,
    pub is_dir:  bool,
    pub size:    u64,
    pub kind:    FileKind,
}

#[derive(Clone, Copy, PartialEq)]
pub enum FileKind {
    Dir, File, Link, Fifo, Char, Block, Socket, Unknown,
}

impl FileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Dir    => "DIR",
            FileKind::File   => "FILE",
            FileKind::Link   => "LINK",
            FileKind::Fifo   => "FIFO",
            FileKind::Char   => "CHAR",
            FileKind::Block  => "BLK",
            FileKind::Socket => "SOCK",
            FileKind::Unknown => "ERR",
        }
    }
}

impl FileEntry {
    fn from_path(path: PathBuf) -> Self {
        use std::os::unix::fs::FileTypeExt;
        let meta = path.symlink_metadata();
        let (is_dir, size, kind) = match meta {
            Err(_) => (false, 0, FileKind::Unknown),
            Ok(m) => {
                let ft = m.file_type();
                let kind = if ft.is_symlink()      { FileKind::Link   }
                      else if ft.is_dir()           { FileKind::Dir    }
                      else if ft.is_fifo()          { FileKind::Fifo   }
                      else if ft.is_char_device()   { FileKind::Char   }
                      else if ft.is_block_device()  { FileKind::Block  }
                      else if ft.is_socket()        { FileKind::Socket }
                      else                          { FileKind::File   };
                (ft.is_dir(), m.len(), kind)
            }
        };
        FileEntry { path, is_dir, size, kind }
    }
}

pub struct FileList {
    entries:        Vec<FileEntry>,
    selected_index: Option<usize>,
}

impl FileList {
    pub fn new() -> Self {
        Self { entries: Vec::new(), selected_index: None }
    }

    pub fn load_dir(&mut self, path: PathBuf) -> io::Result<()> {
        self.entries.clear();
        self.selected_index = None;

        for entry in read_dir(path)? {
            let entry = entry?;
            self.entries.push(FileEntry::from_path(entry.path()));
        }

        self.sort();
        Ok(())
    }

    pub fn sort(&mut self) {
        self.entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.path.file_name().cmp(&b.path.file_name()),
        });
    }

    pub fn select(&mut self, index: usize) -> Result<(), String> {
        if index >= self.entries.len() {
            return Err(format!("SELECTING: index out of bounds {} >= {}", index, self.len()));
        }
        self.selected_index = Some(index);
        Ok(())
    }

    pub fn deselect(&mut self) {
        self.selected_index = None;
    }

    pub fn get_selected_file(&self) -> Result<&PathBuf, String> {
        match self.selected_index {
            Some(i) => Ok(&self.entries[i].path),
            None    => Err("No File Selected!".to_string()),
        }
    }

    pub fn get(&self, index: usize) -> Option<&PathBuf> {
        self.entries.get(index).map(|e| &e.path)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }
}
