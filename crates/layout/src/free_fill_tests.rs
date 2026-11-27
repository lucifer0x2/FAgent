//! FB-23: `fill` children of a `free` container size against that container.
use crate::stack_tests::{input, node, run};
use serde_json::json;

#[test]
fn fill_inside_a_free_frame_takes_the_frame_size() {
    let b = run(input(
        (1440., 400.),
        json!({"mode":"column","padding":[0,20]}),
        json!([
            node("icon", "f", "frame", json!({"width":24,"height":55})),
            node(
                "bg",
                "icon",
                "rectangle",
                json!({"width":"fill","height":"fill"})
            ),
            node(
                "dot",
                "icon",
                "rectangle",
                json!({"x":4,"y":5,"width":"fill","height":10})
            )
        ]),
    ));
    assert_eq!(b["icon"], [20., 0., 24., 55.]);
    assert_eq!(b["bg"], [20., 0., 24., 55.]);
    assert_eq!(b["dot"], [24., 5., 24., 10.]);
}

#[test]
fn fill_inside_a_nested_free_frame_ignores_outer_free_frames() {
    let b = run(input(
        (800., 600.),
        json!({"mode":"free"}),
        json!([
            node(
                "outer",
                "f",
                "frame",
                json!({"x":100,"y":50,"width":300,"height":200})
            ),
            node(
                "inner",
                "outer",
                "frame",
                json!({"x":10,"y":20,"width":40,"height":30})
            ),
            node(
                "fill",
                "inner",
                "rectangle",
                json!({"width":"fill","height":"fill"})
            )
        ]),
    ));
    assert_eq!(b["inner"], [110., 70., 40., 30.]);
    assert_eq!(b["fill"], [110., 70., 40., 30.]);
}
