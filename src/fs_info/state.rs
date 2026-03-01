// File System State Flag
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StateFlag {
    Ready,
    Error,
    Operating,
}

// Statement Manager
pub struct FileSysState {
    flag: StateFlag,
    info: String,
}

impl FileSysState {
    pub fn new() -> Self {
        Self {
            flag: StateFlag::Operating,
            info: "Initializing".to_string(),
        }
    }

    pub fn set<S: Into<String>>(&mut self, flag: StateFlag, info: S) {
        self.flag = flag;
        self.info = info.into();
    }

    pub fn flag(&self) -> StateFlag {
        self.flag
    }

    pub fn info(&self) -> &str {
        &self.info
    }
}
