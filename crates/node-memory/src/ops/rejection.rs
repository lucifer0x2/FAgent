//! A batch the `Txn` rejected, with the public error.
use fagent_scene_input::StorageError;

/// A batch the `Txn` rejected. The error is the one `Candidate::apply` reports for
/// the same batch (same code, field and operation index): every operation's checks
/// run in `Candidate`'s order on the same working state. The end-of-batch node cap
/// carries no operation index, as full validation reports it.
#[derive(Debug)]
pub(crate) struct Rejection {
    /// Boxed so every `Result` on the write path stays small.
    error: Box<StorageError>,
}

impl Rejection {
    /// The same rejection attributed to operation `index`.
    pub(crate) fn at(mut self, index: usize) -> Self {
        self.error.op_index = Some(index);
        self
    }

    pub(crate) fn into_error(self) -> StorageError {
        *self.error
    }
}

impl From<StorageError> for Rejection {
    fn from(error: StorageError) -> Self {
        Self {
            error: Box::new(error),
        }
    }
}
