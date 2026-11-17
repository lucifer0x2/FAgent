//! Every stored row of one project as text, ordered, timestamps and per-database
//! generations left out.
use std::path::Path;

/// SQLite: every authored table, history and replay response of one project file.
pub fn sqlite(db: &Path) -> Vec<String> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let queries = [
        "SELECT 'node',id,coalesce(parent_id,''),sort_key,kind,props_json FROM nodes ORDER BY id",
        "SELECT 'token',id,'',0,'',data_json FROM tokens ORDER BY id",
        "SELECT 'style',id,'',0,'',data_json FROM styles ORDER BY id",
        "SELECT 'note',id,'',0,'',data_json FROM notes ORDER BY id",
        "SELECT 'asset',id,'',0,'',data_json FROM assets ORDER BY id",
        "SELECT 'font',id,'',0,'',data_json FROM fonts ORDER BY id",
        "SELECT 'commit',revision,batch_id,base_revision,summary_json,ops_json FROM commits ORDER BY revision",
        "SELECT 'replay',key,request_hash,committed_revision,'',response_json FROM idempotency ORDER BY key",
        "SELECT 'meta',project_id,slug,current_revision,'','' FROM project_meta",
    ];
    let mut out = vec![];
    for query in queries {
        let mut stmt = conn.prepare(query).unwrap();
        let rows = stmt
            .query_map([], |r| {
                let text = |i: usize| -> rusqlite::Result<String> {
                    let value: rusqlite::types::Value = r.get(i)?;
                    Ok(format!("{value:?}"))
                };
                Ok([0, 1, 2, 3, 4, 5].map(|i| text(i).unwrap()).join("|"))
            })
            .unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

/// PostgreSQL: the project's entity, history and replay rows, plus its revision.
pub fn postgres(url: &str, project: &str) -> Vec<String> {
    let mut client = postgres::Client::connect(url, postgres::NoTls).unwrap();
    let queries = [
        "SELECT kind||'|'||id||'|'||data FROM fagent_runtime_v1.entities WHERE project_id=$1 ORDER BY kind,id",
        "SELECT revision::text||'|'||base_revision||'|'||batch_id||'|'||changed_ids||'|'||operations FROM fagent_runtime_v1.commits WHERE project_id=$1 ORDER BY revision",
        "SELECT key||'|'||request_hash||'|'||response||'|'||coalesce(import_asset,'') FROM fagent_runtime_v1.idempotency WHERE project_id=$1 ORDER BY key",
        "SELECT revision::text||'|'||slug||'|'||oldest_revision FROM fagent_runtime_v1.projects WHERE project_id=$1",
    ];
    let mut out = vec![];
    for query in queries {
        for row in client.query(query, &[&project]).unwrap() {
            out.push(row.get::<_, String>(0));
        }
    }
    out
}
