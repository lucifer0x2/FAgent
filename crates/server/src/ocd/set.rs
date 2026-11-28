//! `@set id {attr}` compiles to `update_node`. Whole values (`fill=`, `layout={…}`,
//! `stroke=c,w,a`, `props={…}`) replace the stored prop. Partial writes of an object prop
//! merge into the stored object at commit (`merge:true`, a JSON merge patch): layout flags
//! and `p`/`gap`/`align`/`justify`, `<prop>.<field>=` (`layout.gap=8`, `stroke.width=2`),
//! instance descendant overrides (`overrides={…}`, `ph.text=`, and the bare `ph="x"` and
//! `ph={…}` of `use`: a bare key that is no catalog prop names a descendant; `btn/label`
//! reaches through a nested instance), and `stroke=color[,width]`. `key=null` removes a prop or field, so nulls always go into the
//! merge patch. A statement with both writes a replacing update for object values, then a
//! merging one.
use super::error::Result;
use super::keys::{effective_key, layout_field};
use super::lexer::Token;
use super::parser::{DirectiveStmt, Part};
use super::props::{self, Target};
use super::value;
use fagent_storage::{merge_props, merges};
use serde_json::{json, Map, Value};

pub fn update(d: &DirectiveStmt) -> Result<Vec<Value>> {
    if d.parts.is_empty() {
        return Err(d.name.error("@set needs at least one attribute"));
    }
    let id = value::id(&d.positional[0])?;
    let (mut whole, mut patch) = (Vec::new(), Map::new());
    for part in &d.parts {
        match partial(part)? {
            Some(fields) => compose(&mut patch, fields),
            None => whole.push(part.clone()),
        }
    }
    let mut replace = props::build(&whole, Target::Update, None)?;
    if patch.is_empty() && !replace.values().any(Value::is_null) {
        return Ok(vec![op(&id, replace, false)]);
    }
    // A prop written whole takes this statement's partial writes before it replaces.
    for key in replace.keys().cloned().collect::<Vec<_>>() {
        if let Some(fields) = patch.remove(&key) {
            merge_props(&mut replace, &Map::from_iter([(key, fields)]));
        }
    }
    // Scalars, lists, references and `null` (a removal) go into the merge patch; records
    // replace in an update of their own, since a merge patch would merge them.
    let (records, values): (Map<_, _>, Map<_, _>) =
        replace.into_iter().partition(|(_, value)| merges(value));
    patch.extend(values);
    let mut ops = Vec::new();
    if !records.is_empty() {
        ops.push(op(&id, records, false));
    }
    if !patch.is_empty() {
        ops.push(op(&id, patch, true));
    }
    Ok(ops)
}

fn op(id: &str, props: Map<String, Value>, merge: bool) -> Value {
    let mut op = json!({"op":"update_node","id":id,"props":props});
    if merge {
        op["merge"] = json!(true);
    }
    op
}

/// Adds one partial write to the statement's merge patch: records merge field by field and
/// the later write wins, including a `null` (applying the patch removes that field).
fn compose(into: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        let nested = merges(&value) && into.get(&key).is_some_and(merges);
        if nested {
            if let (Some(Value::Object(old)), Value::Object(new)) = (into.get_mut(&key), value) {
                compose(old, new);
            }
        } else {
            into.insert(key, value);
        }
    }
}

/// The props one partial write patches, or `None` for a whole value.
fn partial(part: &Part) -> Result<Option<Map<String, Value>>> {
    let single = |target| props::build(std::slice::from_ref(part), target, None).map(Some);
    match part {
        Part::Flag(_) => single(Target::Update),
        // Per descendant, like `ph={…}`: other descendants' overrides stay, `null` removes.
        Part::Attr(key, _) if key.text == "overrides" => single(Target::Update),
        Part::Attr(key, raw) if key.text == "stroke" => {
            // A JSON object, all three list fields or `null` replace; a color[,width] patches.
            let whole = raw.text.starts_with('{')
                || value::is_null(raw)
                || value::split(raw, ',').len() == 3;
            match whole {
                true => Ok(None),
                false => single(Target::Update),
            }
        }
        Part::Attr(key, raw) => match key.text.split_once('.') {
            Some((prefix, field)) => match effective_key(prefix) {
                Some(prop) => field_patch(key, prefix, prop, field, raw).map(Some),
                // `ph.fill=` overrides instance descendant `ph`, read like a `use` attribute.
                None => single(Target::Instance),
            },
            None if layout_field(&key.text).is_some() => single(Target::Update),
            // `ph="x"`, `ph={…}`: as on `use`, a key that is no prop names a descendant.
            None if effective_key(&key.text).is_none() && key.text != "props" => {
                single(Target::Instance)
            }
            None => Ok(None),
        },
        _ => Ok(None),
    }
}

/// `layout.gap=8`, `stroke.width=2`, `layout.padding=8,16`: one field of an object prop.
fn field_patch(
    key: &Token,
    prefix: &str,
    prop: &str,
    field: &str,
    raw: &Token,
) -> Result<Map<String, Value>> {
    let at = key.slice(prefix.chars().count() + 1, None);
    let mut value = value::parse(raw)?;
    for name in field.rsplit('.') {
        let valid = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric());
        if !valid {
            return Err(at.error(format!("`{field}` is not a field name of `{prop}`")));
        }
        value = json!({ name: value });
    }
    Ok(Map::from_iter([(prop.to_owned(), value)]))
}
