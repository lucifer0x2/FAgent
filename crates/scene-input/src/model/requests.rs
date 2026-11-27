use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    CreateNode {
        id: String,
        kind: NodeKind,
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        props: Value,
        #[serde(default)]
        index: Option<usize>,
    },
    UpdateNode {
        id: String,
        props: Value,
        /// `true` merges `props` as a JSON merge patch (`merge_patch`); otherwise each
        /// top-level key replaces the stored value.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        merge: bool,
    },
    ReparentNode {
        id: String,
        #[serde(rename = "parentId")]
        parent_id: String,
        #[serde(default)]
        index: Option<usize>,
    },
    ReorderNode {
        id: String,
        index: usize,
    },
    DeleteSubtree {
        id: String,
    },
    UpsertToken {
        token: Token,
    },
    UpsertStyle {
        style: Style,
    },
    CreateNote {
        id: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<String>,
    },
    ReplyNote {
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author: Option<String>,
    },
    UpdateNote {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved: Option<bool>,
        #[serde(default, rename = "nodeId", skip_serializing_if = "Option::is_none")]
        node_id: Option<String>,
    },
    DeleteNote {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchRequest {
    pub project_id: String,
    /// The revision the edit is based on. Optional; without `strict` it never
    /// rejects for staleness and only scopes the `concurrent` report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
    /// Opt-in optimistic concurrency: reject with `REVISION_CONFLICT` as before
    /// Task28 I3 (exact revision, or the page guard with `pageIds`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict: bool,
    pub idempotency_key: String,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub project_id: String,
    pub committed_revision: u64,
    pub changed_ids: Vec<String>,
    /// Notes removed because the batch deleted their anchor node (also in `changed_ids`),
    /// sorted; omitted when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_note_ids: Vec<String>,
    pub render_status: String,
    pub replayed: bool,
    /// Writes of commits after `expectedRevision` that this batch overwrote (last
    /// writer wins per property), at most 20, sorted by node id and key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent: Option<Vec<ConcurrentWrite>>,
    /// Number of distinct overwritten writes; present with `concurrent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrent_total: Option<usize>,
    /// History after `expectedRevision` is no longer retained (or too large).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub concurrent_unknown: bool,
}

/// One property another commit wrote after the batch's base revision. `key` is a
/// top-level props key, `placement`, `created` or `deleted`; tokens, styles and
/// notes use `token`, `style` and `note` with their id in `nodeId`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct ConcurrentWrite {
    pub node_id: String,
    pub key: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub revision: u64,
    pub base_revision: u64,
    pub batch_id: String,
    pub operations: Value,
    pub changed_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportAssetRequest {
    pub project_id: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    /// Optional font registration, atomically committed with asset metadata.
    pub font: Option<FontRegistration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontRegistration {
    pub id: String,
    pub family: String,
    pub weight: u16,
    pub style: String,
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAssetResult {
    pub asset: Asset,
    pub result: BatchResult,
}

/// 1–`MAX_IMPORT_ITEMS` assets, at most `MAX_IMPORT_BYTES` in all, committed atomically as one
/// revision. A one-item request is the same canonical request as `ImportAssetRequest`.
#[derive(Debug, Clone)]
pub struct ImportAssetsRequest {
    pub project_id: String,
    pub expected_revision: u64,
    pub idempotency_key: String,
    pub items: Vec<ImportAssetItem>,
}

/// One asset of an `ImportAssetsRequest`, with its optional font registration.
#[derive(Debug, Clone)]
pub struct ImportAssetItem {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub font: Option<FontRegistration>,
}

/// The asset of each item, in request order, and the one commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAssetsResult {
    pub assets: Vec<Asset>,
    pub result: BatchResult,
}

impl From<ImportAssetRequest> for ImportAssetsRequest {
    fn from(request: ImportAssetRequest) -> Self {
        Self {
            project_id: request.project_id,
            expected_revision: request.expected_revision,
            idempotency_key: request.idempotency_key,
            items: vec![ImportAssetItem {
                media_type: request.media_type,
                bytes: request.bytes,
                font: request.font,
            }],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMetrics {
    pub queue_wait_us: u64,
    pub read_us: u64,
    pub validate_us: u64,
    pub sql_write_us: u64,
    pub commit_us: u64,
    pub total_us: u64,
    pub request_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub busy: i64,
    pub wal_pages: i64,
    pub checkpointed_pages: i64,
}

/// BTreeMap ordering supplies canonical object-key ordering recursively.
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), canonical_json(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}
