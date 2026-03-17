extern crate alloc;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use postcard::{from_bytes, to_allocvec};
use crate::fs_info::operations::{OperationFS, OperationUnitFS};

const MAGIC: [u8; 4] = *b"HST1";
const VERSION: u8 = 1;
const MAX_HISTORY: usize = 80;

// File format:
//   [MAGIC: 4 bytes][VERSION: 1 byte] | [len: 4 bytes][data: len bytes] | ...
//
// Each record is simply a length-prefixed blob.
// `count` in the struct is an in-memory cache so we avoid re-scanning
// the file on every push().

#[derive(Debug)]
pub struct History {
    file_path: PathBuf,
    available: bool,
    count: usize,
}

impl History {
    pub fn new() -> Self {
        let storage = match Self::get_storage_path() {
            Ok(p) => p,
            Err(_) => return Self::disabled(),
        };

        let path = storage.join("history.bin");

        if Self::init_file(&path).is_err() {
            return Self { file_path: path, available: false, count: 0 };
        }

        // File is freshly initialised (cleared) in init_file, so count is 0.
        Self { file_path: path, available: true, count: 0 }
    }

    /// Append an operation record to the history file.
    pub fn push(
        &mut self,
        operation: OperationFS,
        file_source: PathBuf,
        file_destiny: PathBuf,
    ) -> io::Result<()> {
        if !self.available {
            return Ok(());
        }

        let unit = OperationUnitFS { operation, file_source, file_destiny };

        let encoded = to_allocvec(&unit)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let len = encoded.len() as u32;
        if len == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Empty operation unit"));
        }

        // Evict the oldest record when the limit is reached.
        // Use the cached count to avoid a full file scan.
        if self.count >= MAX_HISTORY {
            self.pop_front()?;
        }

        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file_path)?;

        let mut writer = BufWriter::new(&mut file);
        writer.write_all(&len.to_le_bytes())?;
        writer.write_all(&encoded)?;
        writer.flush()?;

        self.count += 1;
        Ok(())
    }

    /// Remove and return the most-recently pushed record (stack pop from the end).
    pub fn pop(&mut self) -> io::Result<Option<OperationUnitFS>> {
        if !self.available {
            return Ok(None);
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.file_path)?;

        // Scan forward, keeping track of every record's start offset.
        // The last valid record is what we want to remove.
        let result = Self::scan_last(&mut file)?;

        match result {
            None => Ok(None),
            Some((record_start, op)) => {
                file.set_len(record_start)?;
                if self.count > 0 {
                    self.count -= 1;
                }
                Ok(Some(op))
            }
        }
    }
    
    pub fn is_available(&self) -> bool {
        self.available
    }
}

impl History {
    /// # Remove and return the oldest record (queue pop from the front).
    /// Used internally to enforce MAX_HISTORY.
    fn pop_front(&mut self) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.file_path)?;

        // Read the first record to find how many bytes to skip.
        file.seek(SeekFrom::Start(5))?;

        let mut len_buf = [0u8; 4];
        if file.read_exact(&mut len_buf).is_err() {
            // File has no records; nothing to remove.
            return Ok(());
        }
        let r_len = u32::from_le_bytes(len_buf) as usize;
        if r_len == 0 || r_len > 65536 {
            return Ok(());
        }

        // The first record occupies bytes [5 .. 5 + 4 + r_len).
        let first_record_end = 5u64 + 4 + r_len as u64;
        let total = file.metadata()?.len();

        if first_record_end >= total {
            // Only one record (or corrupt); just reset to empty.
            file.set_len(5)?;
            self.count = 0;
            return Ok(());
        }

        // Read the remainder and shift it to start right after the header.
        let remainder_len = (total - first_record_end) as usize;
        let mut buf = vec![0u8; remainder_len];
        file.seek(SeekFrom::Start(first_record_end))?;
        file.read_exact(&mut buf)?;

        file.seek(SeekFrom::Start(5))?;
        file.write_all(&buf)?;
        file.set_len(5 + remainder_len as u64)?;

        if self.count > 0 {
            self.count -= 1;
        }
        Ok(())
    }

    /// Scan the file from the beginning and return the start offset and
    /// decoded value of the last valid record, or None if the file is empty.
    fn scan_last(file: &mut File) -> io::Result<Option<(u64, OperationUnitFS)>> {
        file.seek(SeekFrom::Start(5))?;

        let mut last: Option<(u64, OperationUnitFS)> = None;
        let mut buf = [0u8; 4];

        loop {
            let pos = file.stream_position()?;

            if file.read_exact(&mut buf).is_err() {
                break;
            }
            let r_len = u32::from_le_bytes(buf) as usize;
            if r_len == 0 || r_len > 65536 {
                break;
            }

            let mut data = vec![0u8; r_len];
            if file.read_exact(&mut data).is_err() {
                break;
            }

            if let Ok(op) = from_bytes::<OperationUnitFS>(&data) {
                last = Some((pos, op));
            } else {
                // Corrupt record — stop here; the truncate in pop() will clean it up.
                break;
            }
        }

        Ok(last)
    }

    fn disabled() -> Self {
        Self {
            file_path: PathBuf::from("/dev/null"),
            available: false,
            count: 0,
        }
    }

    /// Create or reset the history file, writing only the header.
    fn init_file(path: &Path) -> io::Result<()> {
        // Try to open an existing file and reset it.
        if let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) {
            file.set_len(0)?;
            file.write_all(&MAGIC)?;
            file.write_all(&[VERSION])?;
            file.flush()?;
            return Ok(());
        }

        // File doesn't exist yet — create it.
        let _ = fs::remove_file(path); // ignore error if it doesn't exist
        let mut file = File::create(path)?;
        file.write_all(&MAGIC)?;
        file.write_all(&[VERSION])?;
        file.flush()?;
        Ok(())
    }

    fn get_storage_path() -> io::Result<PathBuf> {
        let candidates = ["/var/lib/npns", "/mnt/data/npns", "/tmp/npns"];
        for &p in &candidates {
            let path = PathBuf::from(p);
            if fs::create_dir_all(&path).is_ok() {
                return Ok(path);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No writable storage directory found",
        ))
    }
}

#[allow(unused)]
#[cfg(test)]
impl History {
    pub fn with_path(path: PathBuf) -> Self {
        if Self::init_file(&path).is_err() {
            return Self { file_path: path, available: false, count: 0 };
        }
        Self { file_path: path, available: true, count: 0 }
    }
}
