#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::fs::{read_dir};
use anyhow::{anyhow, Result};
use crate::fs_info::file_info::FileInfo;
use crate::fs_info::file_ops::{OpsUnit, Operation};

static MAX_HISTORY_SIZE: usize = 64;

#[derive(PartialEq, Clone, Copy)]
pub enum StatusFlag{
    Ready,
    Error,
    Input,
    Others
}

pub struct FileSys{
    current_dir: PathBuf,
    files: Vec<FileInfo>,
    pub selected_index: Option<usize>,
    pub status_info: String,
    pub status_flag: StatusFlag,
    clipboard: Option<(PathBuf, bool)>,
    ops_history: VecDeque<OpsUnit>
}

impl FileSys{
    pub fn init(start_dir: PathBuf) -> Result<Self> {
        let mut fs = FileSys{
            current_dir: start_dir,
            files: Vec::new(),
            selected_index: None,
            status_info: "Initializing".to_string(),
            status_flag: StatusFlag::Others,
            clipboard: None,
            ops_history: VecDeque::with_capacity(MAX_HISTORY_SIZE)
        };

        fs.refresh()?;
        Ok(fs)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.files.clear();
        for entry in read_dir(&self.current_dir)?{
            let entry = entry?;
            let path = entry.path();
            let metadata = path.metadata()?;

            if let Some(file_name) = path.file_name() {
                self.files.push(FileInfo{
                    name: file_name.to_string_lossy().into_owned(),
                    path,
                    is_dir: metadata.is_dir(),
                    size: metadata.len()
                });
            }
        }

        self.selected_index = None;
        self.files.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                a.is_dir.cmp(&b.is_dir).reverse()
            } else {
                a.name.cmp(&b.name)
            }
        });

        self.status_info = "Ready".to_string();
        self.status_flag = StatusFlag::Ready;

        Ok(())
    }

    pub fn select_current(&mut self, current_index: usize){
        self.selected_index = Some(current_index);
        if current_index < self.files.len() {
            let file = &self.files[current_index];
            self.status_info = format!("Selected: {}", file.name);
            self.status_flag = StatusFlag::Others;
        }
    }

    pub fn copy_selected(&mut self, is_copy: bool) -> Result<()>{
        if let Some(selected_index) = self.selected_index {
            let file = self.files.get(selected_index).cloned().unwrap();
            if !file.is_dir {
                self.clipboard = Some((file.path.clone(), is_copy));
                self.status_info = format!("{}: {}", if is_copy { "Copied" } else { "Cut" }, file.name);
                self.status_flag = StatusFlag::Others;
            } else {
                self.status_info = "Operation Not Supported".to_string();
                self.status_flag = StatusFlag::Error;
            }
        } else {
            self.status_info = "No File Selected".to_string();
            self.status_flag = StatusFlag::Error;
        }
        Ok(())
    }

    pub fn paste(&mut self) -> Result<()>{
        let (source, is_copy) = match &self.clipboard {
            Some((clipboard, is_copy)) => (clipboard.clone(), *is_copy),
            None => {
                self.status_info = "Clipboard is empty".to_string();
                self.status_flag = StatusFlag::Error;
                return Ok(());
            },
        };

        if !source.exists() {
            self.status_info = "Source file does not exist".to_string();
            self.status_flag = StatusFlag::Error;
            self.clipboard = None;
            return Ok(());
        };

        let target_dir = match self.selected_index {
            Some(index) => {
                if self.files.get(index).unwrap().is_dir {
                    self.files.get(index).unwrap().path.clone()
                }  else {
                    self.current_dir.clone()
                }
            }
            None => self.current_dir.clone()
        };

        let file_name = source.file_name().ok_or_else(||anyhow!("Invalid file name"))?;
        let target_path = target_dir.join(file_name);

        if target_path.exists() {
            self.status_info = "File already exists".to_string();
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let op = if is_copy {
            std::fs::copy(&source, &target_path)?;
            OpsUnit {
                operation: Operation::Copy,
                file_source: source.clone(),
                file_target: target_path.clone()
            }
        } else {
            std::fs::rename(&source, &target_path)?;
            OpsUnit {
                operation: Operation::Cut,
                file_source: source.clone(),
                file_target: target_path.clone()
            }
        };

        Self::push_history(&mut self.ops_history, op);
        self.refresh()?;
        self.status_info = format!("Pasted: {}", file_name.to_string_lossy());
        self.status_flag = StatusFlag::Others;
        Ok(())
    }

    pub fn delete_selected(&mut self) -> Result<()>{
        let source = match self.selected_index {
            Some(index) => self.files.get(index).cloned().unwrap().path,
            None => {
                self.status_info = "No Selected".to_string();
                self.status_flag = StatusFlag::Error;
                return Ok(());
            }
        };

        if source.is_dir() {
            std::fs::remove_dir_all(&source)?;
        } else {
            std::fs::remove_file(&source)?;
        }

        self.refresh()?;
        self.status_info = format!("Deleted: {}", source.file_name().unwrap().to_string_lossy());
        self.status_flag = StatusFlag::Others;
        Ok(())
    }

    pub fn new_file(&mut self, name: &str, is_dir: bool) -> Result<()> {
        if validate_filename(&name).is_err() {
            self.status_info = "Invalid Name".to_string();
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let target_dir = match self.selected_index {
            Some(idx) => {
                let selected = &self.files[idx];
                if selected.is_dir {
                    selected.path.join(name)
                } else {
                    self.current_dir.join(name)
                }
            }
            None => self.current_dir.join(name),
        };

        let target_path = target_dir;

        if target_path.exists() {
            self.status_info = format!("{} Exists", name);
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let op = if is_dir {
            std::fs::create_dir(&target_path)?;
            self.status_info = format!("Dir Created: {}", name);
            OpsUnit {
                operation: Operation::New,
                file_source: PathBuf::new(),
                file_target: target_path,
            }
        } else {
            std::fs::File::create(&target_path)?;
            self.status_info = format!("File Created: {}", name);
            OpsUnit {
                operation: Operation::New,
                file_source: PathBuf::new(),
                file_target: target_path,
            }
        };

        Self::push_history(&mut self.ops_history, op);
        self.status_flag = StatusFlag::Others;
        self.refresh()?;
        Ok(())
    }

    pub fn rename_selected(&mut self, new_name: &str) -> Result<()> {
        if validate_filename(new_name).is_err() {
            self.status_info = "Invalid Name".to_string();
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let source = match self.selected_index {
            Some(idx) => self.files.get(idx).cloned().unwrap().path,
            None => {
                self.status_info = "No Selection".to_string();
                self.status_flag = StatusFlag::Error;
                return Ok(());
            }
        };
        let target = self.current_dir.join(new_name);

        if target.exists() {
            self.status_info = format!("{} Exists", new_name);
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let op = OpsUnit {
            operation: Operation::Rename,
            file_source: source.clone(),
            file_target: target.clone(),
        };
        std::fs::rename(&source, &target)?;
        Self::push_history(&mut self.ops_history, op);
        self.refresh()?;
        self.status_info = format!("Renamed to: {}", new_name);
        self.status_flag = StatusFlag::Others;
        Ok(())
    }

    pub fn parent_dir(&mut self) -> Result<()> {
        if let Some(parent) = self.current_dir.parent() {
            let op = OpsUnit {
                operation: Operation::CD,
                file_source: self.current_dir.clone(),
                file_target: parent.to_path_buf(),
            };
            Self::push_history(&mut self.ops_history, op);
            self.current_dir = parent.to_path_buf();
            self.refresh()?;
        } else {
            self.status_info = "Root Dir".to_string();
            self.status_flag = StatusFlag::Error;
        }
        self.selected_index = None;
        Ok(())
    }

    pub fn sub_dir(&mut self, index: usize) -> Result<()> {
        let file = match self.selected_index {
            Some(selected_index) if selected_index == index => self.files.get(index).cloned().unwrap(),
            _ => {
                self.status_info = "No Selection".to_string();
                self.status_flag = StatusFlag::Error;
                return Ok(());
            }
        };

        if !file.is_dir {
            self.status_info = "Not Dir".to_string();
            self.status_flag = StatusFlag::Error;
            return Ok(());
        }

        let op = OpsUnit {
            operation: Operation::CD,
            file_source: self.current_dir.clone(),
            file_target: file.path.clone(),
        };
        Self::push_history(&mut self.ops_history, op);
        self.current_dir = file.path.clone();
        self.refresh()?;
        self.selected_index = None;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        let last_op = match self.ops_history.pop_front() {
            Some(op) => op,
            None => {
                self.status_info = "Nothing to undo".to_string();
                self.status_flag = StatusFlag::Others;
                return Ok(());
            }
        };

        match last_op.operation {
            Operation::Copy => {
                if last_op.file_target.exists() {
                    std::fs::remove_file(&last_op.file_target)?;
                }
            }
            Operation::Cut | Operation::Rename => {
                if last_op.file_target.exists() {
                    std::fs::rename(&last_op.file_target, &last_op.file_source)?;
                }
            }
            Operation::New => {
                if last_op.file_target.exists() {
                    if last_op.file_target.is_dir() {
                        std::fs::remove_dir_all(&last_op.file_target)?;
                    } else {
                        std::fs::remove_file(&last_op.file_target)?;
                    }
                }
            }
            Operation::CD => {
                self.current_dir = last_op.file_source;
                self.refresh()?;
            }
        }
        self.refresh()?;
        self.status_info = "Undone".to_string();
        self.status_flag = StatusFlag::Others;
        Ok(())
    }

    fn push_history(target: &mut VecDeque<OpsUnit>, ops: OpsUnit){
        if target.len() == MAX_HISTORY_SIZE {
            target.pop_back();
        }
        target.push_front(ops)
    }

    pub fn files(&self) -> &Vec<FileInfo> { &self.files }
    pub fn current_dir(&self) -> &PathBuf { &self.current_dir }
    pub fn status_info(&self) -> &str { &self.status_info }
    pub fn status_flag(&self) -> StatusFlag { self.status_flag }
    pub fn selected_index(&self) -> Option<usize> { self.selected_index }
}

fn validate_filename(name: &str) -> Result<(), ()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        Err(())
    } else {
        Ok(())
    }
}