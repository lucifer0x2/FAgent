//! Agent feedback, phase 1 (Task 43, slice A): `key=null` removes a prop on `@set` (FB-19),
//! a bare `clip` flag (FB-37) and root frame heights (FB-5).
use super::tests::ops;
use super::{compile, OcdError};
use serde_json::{json, Value};

fn error(source: &str) -> OcdError {
    compile(source).expect_err(source)
}

fn props(line: &str) -> Value {
    ops(&format!("page#p\n  frame#root 100x100 col\n    {line}\n"))[2]["props"].clone()
}

#[test]
fn null_on_set_removes_props_through_one_merge_patch() {
    assert_eq!(
        ops("@set n maxWidth=null fill=null layout.gap=null t1.fill=null"),
        vec![json!({"op":"update_node","id":"n","merge":true,"props":{
            "maxWidth":null,"fill":null,"layout":{"gap":null},
            "overrides":{"t1":{"fill":null}}}})]
    );
    // Shorthands and literal keys too; a whole descendant override goes with `t1=null`.
    assert_eq!(
        ops("@set n gap=null name=null t1=null stroke.width=null"),
        vec![json!({"op":"update_node","id":"n","merge":true,"props":{
            "layout":{"gap":null},"name":null,"overrides":{"t1":null},"stroke":{"width":null}}})]
    );
    // A whole object still replaces first; the null goes into the merge patch after it.
    assert_eq!(
        ops(r#"@set n layout={"mode":"row"} fill=null"#),
        vec![
            json!({"op":"update_node","id":"n","props":{"layout":{"mode":"row"}}}),
            json!({"op":"update_node","id":"n","merge":true,"props":{"fill":null}}),
        ]
    );
    // The text "null" is quoted.
    assert_eq!(
        ops(r#"@set n text="null""#)[0]["props"],
        json!({"text":"null"})
    );
}

#[test]
fn null_on_a_create_line_or_resource_fails() {
    let cases = [
        ("page#p\n  frame#a 10x10 fill=null\n", (2, 22)),
        ("page#p\n  text#t name=null \"x\"\n", (2, 15)),
        (
            "page#p\n  comp#m 10x10\n    text#t \"x\"\n  use#u m t=null\n",
            (4, 13),
        ),
        ("@style s fill=null\n", (1, 15)),
        ("@token t #000 dark=null\n", (1, 20)),
        ("@token t null\n", (1, 10)),
    ];
    for (source, at) in cases {
        let e = error(source);
        assert_eq!((e.line, e.column), at, "{source}: {}", e.message);
        assert!(
            e.message.starts_with("null only removes a prop on @set"),
            "{}",
            e.message
        );
    }
}

#[test]
fn bare_clip_is_clip_true() {
    assert_eq!(
        props("frame#x w=10 h=10 clip"),
        props("frame#x w=10 h=10 clip=true")
    );
    assert_eq!(props("frame#x clip"), json!({"clip":true}));
    assert_eq!(ops("@set x clip"), ops("@set x clip=true"));
    assert_eq!(
        ops("@set x clip"),
        vec![json!({"op":"update_node","id":"x","props":{"clip":true}})]
    );
    // On @token a bare flag is not a theme name.
    let e = error("@token x #fff clip");
    assert!(
        e.message.starts_with("@token takes theme=value"),
        "{}",
        e.message
    );
    assert_eq!(e.column, 15);
}

#[test]
fn other_bare_prop_names_ask_for_a_value() {
    for (source, key, at) in [
        ("page#p\n  frame#x 10x10 visible\n", "visible", (2, 17)),
        ("@set x opacity\n", "opacity", (1, 8)),
        ("page#p\n  frame#x 10x10 w\n", "w", (2, 17)),
    ] {
        let e = error(source);
        assert_eq!((e.line, e.column), at, "{}", e.message);
        assert!(
            e.message.contains(&format!("write {key}=<value>")),
            "{}",
            e.message
        );
    }
    // Words that are not keys keep the general message.
    let e = error("page#p\n  frame#x 10x10 bogus\n");
    assert!(e.message.starts_with("Unexpected `bogus`"), "{}", e.message);
}

#[test]
fn row_and_column_roots_without_h_hug_their_content() {
    let root = |line: &str| ops(&format!("page#p\n  {line}\n"))[1]["props"].clone();
    assert_eq!(
        root("frame#r w=1440 col"),
        json!({"width":1440,"layout":{"mode":"column"},"height":"hug"})
    );
    assert_eq!(root("frame#r w=1440 row gap=8")["height"], "hug");
    // Written heights stay as written.
    assert_eq!(root("frame#r w=1440 h=900 col")["height"], 900);
    assert_eq!(root("frame#r 1440x900")["height"], 900);
    assert_eq!(root(r#"frame#r w=1440 props={"height":10}"#)["height"], 10);
    // A nested frame and a component root get no height.
    let nested = ops("page#p\n  frame#r 100x100 col\n    frame#n w=10 col\n");
    assert!(nested[2]["props"].get("height").is_none(), "{nested:?}");
    let comp = ops("page#p\n  comp#c w=10 col\n");
    assert!(comp[1]["props"].get("height").is_none(), "{comp:?}");
    // Under `@in` the parent's kind is not known here: nothing is written.
    let within = ops("@in pg\n  frame#r w=10 col\n");
    assert!(within[0]["props"].get("height").is_none(), "{within:?}");
}

/// FB-27: a nested frame written without `h` stores no height, so a stretching row sizes it
/// (layout: `stretch_tests.rs`; end to end: `tests/ocd_stretch.rs`).
#[test]
fn a_nested_free_frame_without_h_gets_no_height() {
    let source = "page#p\n  frame#r w=320 col\n    frame#row w=fill row align=stretch\n      \
        frame#x w=24 free\n      rect#tall 10x55\n";
    let x = &ops(source)[3];
    assert_eq!(x["id"], "x");
    assert_eq!(x["props"], json!({"width":24,"layout":{"mode":"free"}}));
}

#[test]
fn free_roots_without_h_fail_with_the_hint() {
    for source in [
        "page#p\n  frame#r w=1440 free\n",
        "page#p\n  frame#r w=1440\n",
        "page#p\n  frame#r w=1440 col layout={\"mode\":\"free\"}\n",
    ] {
        let e = error(source);
        assert_eq!((e.line, e.column), (2, 3), "{source}");
        assert_eq!(
            e.message, "root frames need h=<n>, or h=hug with row/col",
            "{source}"
        );
    }
}
