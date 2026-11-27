//! How many measured overflow items a response shows. A job's scene stores every item (up to
//! 500, counted beyond that by its summary); a render record and the context reads built on it
//! show the first [`REPORTED`], as before; `inspect_layout` pages the stored list.
use super::budget::{ITEM, SUMMARY};
use serde_json::{json, Value};

/// Overflow items a job record shows, with its summary counting the rest as omitted.
pub(crate) const REPORTED: usize = 12;
/// Largest `diagnosticsLimit` of one `inspect_layout` page.
pub(crate) const MAX_PAGE: usize = 500;

/// A job record's diagnostics from its scene's: every other diagnostic as is, the first
/// [`REPORTED`] overflow items, and the summary with `reportedNodes`/`omittedNodes` matching.
pub(crate) fn reported(scene: &Value) -> Value {
    let Some(list) = scene.as_array() else {
        return scene.clone();
    };
    let items = list.iter().filter(|d| d["code"] == ITEM).count();
    if items <= REPORTED {
        return scene.clone();
    }
    let dropped = (items - REPORTED) as u64;
    let mut seen = 0;
    let mut out = Vec::with_capacity(list.len());
    for diagnostic in list {
        if diagnostic["code"] == ITEM {
            seen += 1;
            if seen > REPORTED {
                continue;
            }
        }
        let mut diagnostic = diagnostic.clone();
        if diagnostic["code"] == SUMMARY {
            diagnostic["reportedNodes"] = json!(REPORTED);
            diagnostic["omittedNodes"] =
                json!(diagnostic["omittedNodes"].as_u64().unwrap_or(0) + dropped);
        }
        out.push(diagnostic);
    }
    Value::Array(out)
}

/// One page of overflow items: every other diagnostic stays, items `offset..offset+limit`
/// remain. Returns the page metadata when the items do not all fit on this page.
pub(crate) fn page(diagnostics: &mut Vec<Value>, offset: usize, limit: usize) -> Option<Value> {
    let total = items(diagnostics);
    if offset == 0 && total <= limit {
        return None;
    }
    // The offset is caller input with no upper bound: saturate instead of overflowing.
    let end = offset.saturating_add(limit);
    let mut index = 0;
    diagnostics.retain(|d| {
        if d["code"] != ITEM {
            return true;
        }
        index += 1;
        index > offset && index <= end
    });
    let next = (end < total).then_some(end);
    Some(json!({"offset":offset,"limit":limit,"total":total,"nextOffset":next}))
}

/// Overflow items in `diagnostics`.
pub(crate) fn items(diagnostics: &[Value]) -> usize {
    diagnostics.iter().filter(|d| d["code"] == ITEM).count()
}

/// Page metadata once a byte budget kept the first `kept` of the `shown` items of a page:
/// `nextOffset` resumes after the last item kept, so a client following it sees every item.
/// A budget that keeps none leaves the metadata as is; its summary says how to read them.
pub(crate) fn resume(
    meta: Option<Value>,
    offset: usize,
    limit: usize,
    shown: usize,
    kept: usize,
) -> Option<Value> {
    if kept == 0 || kept >= shown {
        return meta;
    }
    let mut meta = meta.unwrap_or_else(|| json!({"offset":offset,"limit":limit,"total":shown}));
    meta["nextOffset"] = json!(offset + kept);
    Some(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(items: usize) -> Value {
        let mut list: Vec<Value> = (0..items)
            .map(|i| json!({"code":ITEM,"nodeId":format!("t{i}")}))
            .collect();
        list.insert(0, json!("STATE_NOT_DECLARED"));
        list.push(json!({"code":SUMMARY,"reportedNodes":items,"omittedNodes":0}));
        Value::Array(list)
    }

    #[test]
    fn records_show_twelve_items_and_count_the_rest() {
        assert_eq!(reported(&job(12)), job(12));
        let shown = reported(&job(20));
        let list = shown.as_array().unwrap();
        assert_eq!(list.len(), 1 + 12 + 1);
        assert_eq!(list[0], "STATE_NOT_DECLARED");
        assert_eq!(list[12]["nodeId"], "t11");
        assert_eq!(list[13]["reportedNodes"], 12);
        assert_eq!(list[13]["omittedNodes"], 8);
    }

    #[test]
    fn pages_cover_every_item_once() {
        let mut first = job(20).as_array().unwrap().clone();
        assert_eq!(
            page(&mut first, 0, 12),
            Some(json!({"offset":0,"limit":12,"total":20,"nextOffset":12}))
        );
        let mut second = job(20).as_array().unwrap().clone();
        let meta = page(&mut second, 12, 12).unwrap();
        assert_eq!(meta["nextOffset"], Value::Null);
        let ids: Vec<_> = first
            .iter()
            .chain(&second)
            .filter_map(|d| d["nodeId"].as_str())
            .collect();
        assert_eq!(ids.len(), 20);
        assert_eq!(
            second.len(),
            1 + 8 + 1,
            "other diagnostics and the summary stay"
        );
        let mut whole = job(3).as_array().unwrap().clone();
        assert_eq!(page(&mut whole, 0, 12), None);
        assert_eq!(whole.len(), 5);
    }

    #[test]
    fn an_offset_past_every_item_shows_none() {
        let mut list = job(3).as_array().unwrap().clone();
        let meta = page(&mut list, usize::MAX, 12).unwrap();
        assert_eq!(meta["nextOffset"], Value::Null);
        assert_eq!(
            list.len(),
            2,
            "only the other diagnostic and the summary stay"
        );
    }
}
