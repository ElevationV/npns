use std::path::PathBuf;
use serde::{Serialize, Deserialize};

/// File System Operation Unit
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationUnitFS {
    pub operation: OperationFS,
    pub file_source: PathBuf,
    pub file_destiny: PathBuf,
}

/// File System Operations supported
#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum OperationFS {
    None,
    Copy,
    Move,
    Rename,
    New,
    ChangeDir,
    Remove,
    StartRange,
    EndRange
}
