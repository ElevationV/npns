use std::path::PathBuf;

pub enum Operation {
    Copy,
    Cut,
    Rename,
    New,
    CD,
}

pub struct OpsUnit{
    pub operation: Operation,
    pub file_source: PathBuf,
    pub file_target: PathBuf,
}
