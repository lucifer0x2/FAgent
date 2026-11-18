//! Admission at load: the full reference validation of a stored revision, once.
//! Stored data that fails it is still loaded and served; the failure is counted and
//! logged, and later commits are validated like on any other revision (see
//! `fagent_scene_input::validate::memory::admit` for when they stay local).
use super::{nanos, NodeMemory, Stats};
use crate::ProjectHandle;
use fagent_scene_input::validate::memory;
use fagent_scene_input::StorageError;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

impl NodeMemory {
    /// Validates the loaded revision of `handle` and certifies it for delta
    /// validation. Never fails the load.
    pub(super) fn admit(&self, handle: &ProjectHandle) {
        let start = nanos(&*self.clock);
        let result = memory::admit(&handle.read_view());
        let micros = nanos(&*self.clock).saturating_sub(start) / 1_000;
        self.stats
            .admission_micros
            .fetch_add(micros, Ordering::Relaxed);
        Stats::bump(&self.stats.admissions);
        if let Err(error) = result {
            Stats::bump(&self.stats.admission_failures);
            eprintln!("{}", event(handle.id(), &error));
        }
    }
}

/// The log line of a failed admission: ids and codes only, never the error message
/// or property content.
pub(crate) fn event(project: &str, error: &StorageError) -> Value {
    json!({
        "event": "node_memory_admission",
        "project": project,
        "code": error.code,
        "field": error.field,
        "node": error.node.as_ref().and_then(|n| n.node_id.as_deref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_names_ids_and_codes_only() {
        let error = StorageError::validation("width", "Expected a number, got secret text")
            .on_node("n1", "frame");
        let line = event("p1", &error);
        assert_eq!(
            line,
            json!({"event":"node_memory_admission","project":"p1","code":"VALIDATION_FAILED",
                "field":"width","node":"n1"})
        );
        assert!(!line.to_string().contains("secret"));
    }
}
