//! Test helper: where two read responses first differ.
use serde_json::Value;

/// The JSON path of the first difference, e.g. `/nodes/3/id`.
pub(crate) fn first_difference(a: &Value, b: &Value, path: String) -> String {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys_x: Vec<_> = x.keys().collect();
            let keys_y: Vec<_> = y.keys().collect();
            if keys_x != keys_y {
                return format!("{path} (keys)");
            }
            x.iter()
                .find(|(k, v)| y.get(k.as_str()) != Some(*v))
                .map(|(k, v)| first_difference(v, &y[k.as_str()], format!("{path}/{k}")))
                .unwrap_or(format!("{path} (order)"))
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .position(|(v, w)| v != w)
            .map(|i| first_difference(&x[i], &y[i], format!("{path}/{i}")))
            .unwrap_or(format!("{path} (bytes)")),
        (Value::Array(x), Value::Array(y)) => {
            format!("{path} (length {} vs {})", x.len(), y.len())
        }
        _ => path,
    }
}
