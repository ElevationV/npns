/// Check Result, set Error state and return if it's Err.
/// Use at the end of the error chain, not in the middle of propagation.
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

/// Check Result, set Error state and return Err(PasteAbort::Error) if it's Err.
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
