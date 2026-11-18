//! Why a batch was rejected, and whether that error is the public one.
use fagent_scene_input::StorageError;

/// A batch the `Txn` rejected.
///
/// `exact` errors are the ones `Candidate::apply` reports for the same batch (same
/// code, field and operation index): the operation's checks run in `Candidate`'s
/// order on the same working state. The others come from checks `Candidate` makes
/// at another point (the descendant depth of a reparented subtree, the token set,
/// the end-of-batch node cap and component masters), or follow a token upsert, and
/// are replaced by the reference's error.
#[derive(Debug)]
pub(crate) struct Rejection {
    /// Boxed so every `Result` on the write path stays small.
    error: Box<StorageError>,
    pub(crate) exact: bool,
}

impl Rejection {
    /// An error `Candidate` would not report at this point.
    pub(crate) fn inexact(error: StorageError) -> Self {
        Self {
            error: Box::new(error),
            exact: false,
        }
    }

    /// The same error, no longer taken as `Candidate`'s.
    pub(crate) fn routed(mut self) -> Self {
        self.exact = false;
        self
    }

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
            exact: true,
        }
    }
}
