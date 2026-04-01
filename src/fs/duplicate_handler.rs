use std::path::PathBuf;

#[derive(Clone)]
pub enum DuplicatedFileHandleOps {
    Overwrite,      // for file
    Rename(String), // for dir/file
    Skip,           // for dir/file
    WriteIn,        // for dir
    Cancel,
    None            // no duplicate
}

// maybe not supposed to be here
pub enum PasteAbort {
    Error,
    Cancel,
}


/// # ApplyToAll
///
/// ## Tracks the "apply to all" state across the pasting_dir loop
///
/// ### Rules:
///   - Rename never enters apply-to-all
///   - A File handler only suppresses callbacks for subsequent FILE conflicts
///   - A Dir  handler only suppresses callbacks for subsequent DIR  conflicts
///   - When conflict type switches (file -> dir or dir -> file) the stored handler does not apply; 
///     user should be asked again and the result becomes the new ApplyToAll state
#[derive(Clone)]
pub enum ApplyToAll {
    No,
    File(DuplicatedFileHandleOps), // applies to file conflicts only
    Dir(DuplicatedFileHandleOps),  // applies to dir  conflicts only
}

impl ApplyToAll {
    // If the stored handler matches `is_dir`, return it; otherwise None
    pub fn get(&self, is_dir: bool) -> Option<&DuplicatedFileHandleOps> {
        match self {
            ApplyToAll::File(h) if !is_dir => Some(h),
            ApplyToAll::Dir(h)  if  is_dir => Some(h),
            _ => None,
        }
    }

    // Update after a user callback returned `(handler, apply)`
    // Rename always resets to No.
    pub fn update(&mut self, handler: &DuplicatedFileHandleOps, apply: bool, is_dir: bool) {
        if !apply || matches!(handler, DuplicatedFileHandleOps::Rename(_)) {
            *self = ApplyToAll::No;
        } else if is_dir {
            *self = ApplyToAll::Dir(handler.clone());
        } else {
            *self = ApplyToAll::File(handler.clone());
        }
    }
}

//FileConflictResult / DirConflictResult
//
// Return types from the two duplicate-handler free functions
// They carry both the resolved destination path and the handler chosen by user, 
// so that pasting_dir can update ApplyToAll correctly
pub enum FileConflictResult {
    // Continue with this destination path; handler recorded for history
    Proceed {
        dest:    PathBuf,
        handler: DuplicatedFileHandleOps,
        apply:   bool,
    },
    Skip { apply: bool },
    Cancel,
}

pub enum DirConflictResult {
    // Continue with this destination path
    Proceed {
        dest:    PathBuf,
        handler: DuplicatedFileHandleOps,
        apply:   bool,
    },
    Skip { apply: bool },
    Cancel,
}


// handle_file_duplicate, handle_dir_duplicate
//
// They are the ONLY places the callback-function is called 
// apart from paste_dir single top-level call, which also goes through handle_dir_duplicate
//
// Both functions loop until the chosen is conflict-free, 
// handling the case where a user-supplied Rename target also collides

// Resolve a file conflict.  Loops on Rename if the new name also collides
pub fn handle_file_duplicate<F>(
    dest: PathBuf,
    callback: &F,
) -> FileConflictResult
where
    F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
{
    let mut current = dest;

    loop {
        if !current.exists() {
            // No (more) conflict — proceed as-is
            return FileConflictResult::Proceed {
                dest:    current,
                handler: DuplicatedFileHandleOps::None,
                apply:   false,
            };
        }

        let (handler, apply) = callback(&current, false);

        match handler {
            DuplicatedFileHandleOps::Rename(ref new_name) => {
                // Build the renamed path and loop to check for another conflict
                let parent = match current.parent() {
                    Some(p) => p.to_path_buf(),
                    None    => return FileConflictResult::Cancel,
                };
                let renamed = parent.join(new_name);
                // Rename never participates in apply-to-all (enforced inApplyToAll::update), 
                current = renamed;
                // Loop — check whether the renamed target also conflicts
            }
            DuplicatedFileHandleOps::Skip => {
                return FileConflictResult::Skip { apply };
            }
            DuplicatedFileHandleOps::Cancel => {
                return FileConflictResult::Cancel;
            }
            // Overwrite / None / WriteIn — proceed with the current path
            other => {
                return FileConflictResult::Proceed {
                    dest:    current,
                    handler: other,
                    apply,
                };
            }
        }
    }
}

// Resolve a directory conflict.  Loops on Rename if the new name also collides
pub fn handle_dir_duplicate<F>(
    dest: PathBuf,
    callback: &F,
) -> DirConflictResult
where
    F: Fn(&PathBuf, bool) -> (DuplicatedFileHandleOps, bool),
{
    let mut current = dest;

    loop {
        if !current.exists() {
            return DirConflictResult::Proceed {
                dest:    current,
                handler: DuplicatedFileHandleOps::None,
                apply:   false,
            };
        }

        let (handler, apply) = callback(&current, true);

        match handler {
            DuplicatedFileHandleOps::Rename(ref new_name) => {
                let parent = match current.parent() {
                    Some(p) => p.to_path_buf(),
                    None    => return DirConflictResult::Cancel,
                };
                current = parent.join(new_name);
                // Loop to verify the renamed target is also conflict-free
            }
            DuplicatedFileHandleOps::Skip => {
                return DirConflictResult::Skip { apply };
            }
            DuplicatedFileHandleOps::Cancel => {
                return DirConflictResult::Cancel;
            }
            // WriteIn / None — proceed with current path (merge into existing dir)
            other => {
                return DirConflictResult::Proceed {
                    dest:    current,
                    handler: other,
                    apply,
                };
            }
        }
    }
}
