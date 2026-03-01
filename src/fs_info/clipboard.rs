use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Clipboard(Option<(PathBuf, bool)>);

impl Clipboard {
    pub fn new() -> Self {
        Self(None)
    }

    pub fn set(&mut self, path: PathBuf, is_copy: bool) {
        self.0 = Some((path, is_copy))
    }


    pub fn get(&self) -> Option<&(PathBuf, bool)> {
        self.0.as_ref()
    }
}