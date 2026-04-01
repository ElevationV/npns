/// Check Result, set Error state and return if it's Err
/// 
/// This macro should be used **at the end of the error chain** where errors are finally handled, 
/// NOT in the middle of error propagation. 
/// because it will return the current function
/// 
/// # Example
/// ```
/// let file = check_or_return!(self, "loading file", some_operation());
/// // if some_operation() returns Err, it will automatically 
/// // set Error state and return from the current function
/// ```
#[macro_export]
macro_rules! check_or_return {
    ($self:expr, $context:expr, $result:expr) => {
        match $result {
            Ok(val) => val,
            Err(err) => {
                $self.set_state(Error, format!("{}: {}", $context, err));
                return;
            }
        }
    };
}

/// check Result and return value if it's Err
/// 
/// won't set error state
/// 
/// ```
/// let file = check_or_return_with!(self, "loading file", some_operation(), false);
/// // set state and return false if it's Err
/// ```
#[macro_export]
macro_rules! check_or_return_with {
    ($self:expr, $result:expr, $return_val:expr) => {
        match $result {
            Ok(val) => val,
            Err(_) => {
                return $return_val;
            }
        }
    };
}

/// check Result，set Error state and return Err(PasteAbort::Error) if it's Err
/// 
/// a specified version of `check_or_return_with`
/// used to mark the state of `paste_dir()`
#[macro_export]
macro_rules! check_or_abort_paste {
    ($self:expr, $msg:expr, $expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(err) => {
                $self.set_state(Error, format!("{}: {}", $msg, err));
                return Err(PasteAbort::Error);
            }
        }
    };
}
