//! Note limits: compiled defaults that one process may raise once at startup (the server
//! reads `FAGENT_MAX_NOTE*`). Every note check reads [`note_limits`].
use crate::{Result, StorageError};
use std::sync::OnceLock;

/// Per-project note limits. Text and author are counted in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteLimits {
    /// Notes per project.
    pub notes: usize,
    /// Characters of one note or reply text.
    pub text: usize,
    /// Replies per note.
    pub replies: usize,
    /// Characters of one author.
    pub author: usize,
}

impl NoteLimits {
    /// The compiled defaults. Only the Task24 benchmark feature raises the note count.
    pub const DEFAULT: Self = Self {
        #[cfg(not(feature = "large-project-bench"))]
        notes: 20_000,
        #[cfg(feature = "large-project-bench")]
        notes: 1_000_000,
        text: 2000,
        replies: 100,
        author: 64,
    };
    /// The highest values a process may configure.
    pub const CEILING: Self = Self {
        notes: 1_000_000,
        text: 100_000,
        replies: 1000,
        author: 256,
    };

    /// Note or reply text: 1 to `text` characters.
    pub fn check_text(&self, text: &str) -> Result<()> {
        match (1..=self.text).contains(&text.chars().count()) {
            true => Ok(()),
            false => Err(invalid(
                "text",
                &format!("Note text must be 1–{} characters", self.text),
            )),
        }
    }

    /// An absent author is valid; a present one has 1 to `author` characters.
    pub fn check_author(&self, author: Option<&str>) -> Result<()> {
        match author {
            Some(a) if !(1..=self.author).contains(&a.chars().count()) => Err(invalid(
                "author",
                &format!("Note author must be 1–{} characters", self.author),
            )),
            _ => Ok(()),
        }
    }

    /// `reply_note` on a note that already holds `replies` replies.
    pub fn replies_full(&self) -> StorageError {
        invalid(
            "replies",
            &format!(
                "Note already has {} replies; create a new note",
                self.replies
            ),
        )
    }

    /// A stored note with more than `replies` replies.
    pub fn too_many_replies(&self) -> StorageError {
        invalid("replies", &format!("Note exceeds {} replies", self.replies))
    }

    /// A project with more than `notes` notes.
    pub fn too_many_notes(&self) -> StorageError {
        invalid("notes", &format!("Project exceeds {} notes", self.notes))
    }
}

static CONFIGURED: OnceLock<NoteLimits> = OnceLock::new();

/// The limits of this process: the configured ones, else [`NoteLimits::DEFAULT`].
pub fn note_limits() -> NoteLimits {
    CONFIGURED.get().copied().unwrap_or(NoteLimits::DEFAULT)
}

/// Sets this process's limits once, before any project is opened. Each value must lie
/// between the default and the ceiling, so data valid under the defaults stays valid.
pub fn configure_note_limits(limits: NoteLimits) -> std::result::Result<(), &'static str> {
    let (low, high) = (NoteLimits::DEFAULT, NoteLimits::CEILING);
    let within = |value: usize, low: usize, high: usize| (low..=high).contains(&value);
    if !(within(limits.notes, low.notes, high.notes)
        && within(limits.text, low.text, high.text)
        && within(limits.replies, low.replies, high.replies)
        && within(limits.author, low.author, high.author))
    {
        return Err("Note limits must lie between the defaults and the ceilings");
    }
    if CONFIGURED.get_or_init(|| limits) != &limits {
        return Err("Note limits are already configured");
    }
    Ok(())
}

fn invalid(field: &str, message: &str) -> StorageError {
    StorageError::validation(field, message)
}

#[cfg(test)]
mod tests {
    use super::NoteLimits;

    #[test]
    fn messages_name_the_limits_in_use() {
        let raised = NoteLimits {
            notes: 30_000,
            text: 5000,
            replies: 200,
            author: 80,
        };
        let long = "á".repeat(3000);
        assert!(NoteLimits::DEFAULT.check_text(&long).is_err());
        assert!(raised.check_text(&long).is_ok());
        let error = raised.check_text(&"a".repeat(5001)).unwrap_err();
        assert_eq!(error.message, "Note text must be 1–5000 characters");
        assert!(raised.check_text("").is_err());
        assert!(raised.check_author(None).is_ok());
        assert!(raised.check_author(Some(&"b".repeat(80))).is_ok());
        let error = raised.check_author(Some(&"b".repeat(81))).unwrap_err();
        assert_eq!(error.message, "Note author must be 1–80 characters");
        assert_eq!(
            raised.replies_full().message,
            "Note already has 200 replies; create a new note"
        );
        assert_eq!(
            raised.too_many_replies().message,
            "Note exceeds 200 replies"
        );
        assert_eq!(
            raised.too_many_notes().message,
            "Project exceeds 30000 notes"
        );
    }

    #[test]
    fn defaults_keep_the_published_messages() {
        let limits = NoteLimits::DEFAULT;
        let error = limits.check_text(&"a".repeat(2001)).unwrap_err();
        assert_eq!(error.message, "Note text must be 1–2000 characters");
        assert_eq!(error.field.as_deref(), Some("text"));
        let error = limits.check_author(Some("")).unwrap_err();
        assert_eq!(error.message, "Note author must be 1–64 characters");
        assert_eq!(
            limits.replies_full().message,
            "Note already has 100 replies; create a new note"
        );
    }
}
