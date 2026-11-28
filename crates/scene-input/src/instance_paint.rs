//! Patch semantics shared by expansion and full merged-value validation: how an instance's
//! own props and an `overrides.<id>` patch apply to the master node they change.
use serde_json::{Map, Value};

/// The props of an instance's expanded root: the master root's props without `component`,
/// patched by the instance's own props (all but `componentId` and `overrides`).
/// `None` when either side is not an object.
pub(crate) fn instance_root_props(master: &Value, instance: &Value) -> Option<Value> {
    let mut root = master.clone();
    root.as_object_mut()?.remove("component");
    let mut patch = instance.as_object()?.clone();
    patch.remove("componentId");
    patch.remove("overrides");
    merge_props(&mut root, &patch);
    Some(root)
}

/// Applies `patch` to `base`: `stroke` and `layout` merge key by key (a `layout` patch's
/// `breakpoints` replace the master's whole list), `states` merge per state, the rest
/// replace.
pub(crate) fn merge_props(base: &mut Value, patch: &Map<String, Value>) {
    for (key, value) in patch {
        match key.as_str() {
            "stroke" | "layout" => merge_object(&mut base[key], value),
            "states" => {
                if let Some(states) = value.as_object() {
                    if !base[key].is_object() {
                        base[key] = serde_json::json!({});
                    }
                    for (state, props) in states {
                        if let Some(props) = props.as_object() {
                            if !base[key][state].is_object() {
                                base[key][state] = serde_json::json!({});
                            }
                            merge_props(&mut base[key][state], props);
                        } else {
                            base[key][state] = props.clone();
                        }
                    }
                } else {
                    base[key] = value.clone();
                }
            }
            _ => base[key] = value.clone(),
        }
    }
}

/// Merges two override sets of one master: per descendant, `outer` patches on top of
/// `inner` with [`merge_props`], so the outer instance wins key by key.
pub(crate) fn merge_overrides(inner: &mut Map<String, Value>, outer: Map<String, Value>) {
    for (target, patch) in outer {
        match (inner.get_mut(&target), patch) {
            (Some(base @ Value::Object(_)), Value::Object(patch)) => merge_props(base, &patch),
            (_, patch) => {
                inner.insert(target, patch);
            }
        }
    }
}

fn merge_object(base: &mut Value, patch: &Value) {
    if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
        base.extend(patch.clone());
    } else {
        *base = patch.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layout_merges_per_key_and_breakpoints_replace_whole() {
        let mut base = json!({"layout":{"mode":"row","gap":8,"padding":12,
            "breakpoints":[{"maxWidth":400,"layout":{"mode":"column"}}]},"width":120});
        let patch = json!({"layout":{"justify":"center","breakpoints":[]},"width":164});
        merge_props(&mut base, patch.as_object().unwrap());
        assert_eq!(
            base,
            json!({"layout":{"mode":"row","gap":8,"padding":12,"justify":"center",
                "breakpoints":[]},"width":164})
        );
    }

    #[test]
    fn root_props_drop_component_and_instance_only_keys() {
        let master = json!({"component":{"name":"B"},"fill":"#000","layout":{"mode":"row"}});
        let instance = json!({"componentId":"m","overrides":{"t":{"text":"x"}},
            "layout":{"padding":4}});
        assert_eq!(
            instance_root_props(&master, &instance).unwrap(),
            json!({"fill":"#000","layout":{"mode":"row","padding":4}})
        );
    }

    #[test]
    fn outer_overrides_win_per_key() {
        let mut inner = json!({"t":{"text":"inner","fill":"#000"},"u":{"visible":false}});
        let outer = json!({"t":{"text":"outer"},"v":{"text":"new"}});
        merge_overrides(
            inner.as_object_mut().unwrap(),
            outer.as_object().unwrap().clone(),
        );
        assert_eq!(
            inner,
            json!({"t":{"text":"outer","fill":"#000"},"u":{"visible":false},
                "v":{"text":"new"}})
        );
    }
}
