//! Strict memory reads (`FAGENT_NODE_MEMORY_READS=strict`, tests only): a read
//! served from a memory slice must match today's path byte for byte.
use crate::error::Result;
use serde_json::Value;

/// Panics naming the tool and the first differing JSON path, never the bodies.
pub(super) fn same(name: &str, memory: &Result<Value>, today: &Result<Value>) {
    let (a, b) = (outcome(memory), outcome(today));
    if serde_json::to_vec(&a).ok() != serde_json::to_vec(&b).ok() {
        panic!(
            "node-memory read differs: {name} at {}",
            first_difference(&a, &b, String::new())
        );
    }
}

fn outcome(result: &Result<Value>) -> Value {
    match result {
        Ok(value) => value.clone(),
        Err(error) => error.value(),
    }
}

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
