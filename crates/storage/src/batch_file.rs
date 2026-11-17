//! Bulk input is data only. It never executes code or divides a request into commits.
//! A staged file is either a pure JSON operation array or, when its name ends in `.ocd`,
//! UTF-8 OCD source that the MCP server compiles exactly like inline `source`.
use crate::inbox::{self, Purpose};
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchFileRequest {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
    pub idempotency_key: String,
    /// An inbox-relative filename, or an absolute filename inside this project's inbox.
    pub path: String,
}

/// What a staged batch file holds. Only a `.ocd` name (any case) selects source; every
/// other name is read as a JSON operation array, as before source files existed.
#[derive(Debug, Clone, PartialEq)]
pub enum BatchFile {
    Operations(Value),
    Source(String),
}

impl BatchFileRequest {
    pub fn with_operations(&self, operations: Value) -> Result<BatchRequest> {
        Ok(BatchRequest {
            project_id: self.project_id.clone(),
            expected_revision: self.expected_revision,
            strict: self.strict,
            idempotency_key: self.idempotency_key.clone(),
            operations: serde_json::from_value(operations)?,
        })
    }
    /// Whether `path` names an OCD source file.
    pub fn is_source(&self) -> bool {
        let bytes = self.path.as_bytes();
        bytes.len() > 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b".ocd")
    }
}

/// Parses staged bytes by the request's file kind. Source must be UTF-8; a leading byte
/// order mark is dropped so line 1 column 1 is the first visible character.
pub(crate) fn parse(request: &BatchFileRequest, bytes: Vec<u8>) -> Result<BatchFile> {
    if request.is_source() {
        let text = String::from_utf8(bytes)
            .map_err(|_| StorageError::validation("path", "OCD batch file must be UTF-8 text"))?;
        let text = match text.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_owned(),
            None => text,
        };
        return Ok(BatchFile::Source(text));
    }
    let operations: Value = serde_json::from_slice(&bytes).map_err(|_| {
        StorageError::validation(
            "operations",
            "Batch file must contain a pure JSON operation array (name OCD source files *.ocd)",
        )
    })?;
    if !operations
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.len() <= MAX_BATCH_OPS)
    {
        return Err(StorageError::validation(
            "operations",
            "Batch requires a JSON array of 1–500 operations",
        ));
    }
    Ok(BatchFile::Operations(operations))
}

/// The one staged-file read every engine uses: the request checks, then one file from
/// `<root>/<slug>/inbox` under the inbox containment rules and the 2 MiB cap, parsed by
/// kind. `root` must be canonical and `project` the project directory below it.
pub(crate) fn read_staged(
    root: &Path,
    project: &Path,
    project_id: &str,
    request: &BatchFileRequest,
) -> Result<BatchFile> {
    crate::project::request_ids(project_id, &request.project_id, &request.idempotency_key)?;
    let bytes = inbox::read(root, project, &request.path, Purpose::Batch)?;
    parse(request, bytes)
}

/// The JSON operations of a staged file; a `.ocd` file is rejected rather than read as JSON.
pub(crate) fn operations(file: BatchFile) -> Result<Value> {
    match file {
        BatchFile::Operations(operations) => Ok(operations),
        BatchFile::Source(_) => Err(source_unsupported()),
    }
}

/// Storage applies only JSON operations; OCD source is compiled by the MCP server.
pub(crate) fn source_unsupported() -> StorageError {
    StorageError::validation(
        "path",
        "OCD batch files are compiled by the MCP apply_batch_file tool, not by storage",
    )
}

impl Project {
    /// Reads one staged batch file under the inbox containment rules and the 2 MiB cap.
    /// JSON stays untyped until the MCP layer validates the original operation shapes
    /// against its published schema, before serde fills any optional fields.
    pub fn read_batch_file(&self, request: &BatchFileRequest) -> Result<BatchFile> {
        self.check_paths()?;
        read_staged(&self.root.path, &self.path, &self.project_id, request)
    }
    /// The JSON operations of a staged file. Source files compile in the server, so a
    /// `.ocd` path is rejected here rather than read as JSON.
    pub fn read_batch_file_operations(&self, request: &BatchFileRequest) -> Result<Value> {
        operations(self.read_batch_file(request)?)
    }
    pub fn apply_batch_file(&self, request: BatchFileRequest) -> Result<BatchResult> {
        let operations = self.read_batch_file_operations(&request)?;
        self.apply_batch(request.with_operations(operations)?)
    }
    pub fn dry_run_batch_file(&self, request: BatchFileRequest) -> Result<DryRunResult> {
        let operations = self.read_batch_file_operations(&request)?;
        self.dry_run_batch(request.with_operations(operations)?)
    }
}
