//! Agent feedback, phase 1 (Task 43, slice A), end to end on temp SQLite storage: bare
//! descendant keys on `@set` (FB-17, FB-35), `key=null` removals (FB-19), a bare `clip`
//! (FB-37) and root frames without `h` (FB-5).
#[allow(dead_code)]
mod support;
use serde_json::{json, Value};
use support::Server;

const SEED: &str = "page#p
  comp#m 200x40 row
    text#t1 font=font_bundled size=14/20 \"One\"
    text#t2 font=font_bundled size=14/20 \"Two\"
  frame#card 400x300 col gap=8 maxWidth=600 fill=#FFFFFF
    use#x m t2.fill=#111111
";

struct Session {
    s: Server,
    p: String,
    revision: u64,
}

impl Session {
    async fn seeded() -> Self {
        let s = Server::start().await;
        let p = s.project().await;
        let mut session = Self { s, p, revision: 0 };
        let seed = session.apply(SEED).await;
        assert_eq!(seed["committedRevision"], 1, "{seed}");
        session
    }

    /// Applies `source` at the current revision; a commit advances it.
    async fn apply(&mut self, source: &str) -> Value {
        let args = json!({"projectId":self.p,"expectedRevision":self.revision,
            "idempotencyKey":format!("k{}-{}", self.revision, source.len()),"source":source});
        let result = self.s.call("apply_batch", args).await;
        if let Some(revision) = result["committedRevision"].as_u64() {
            self.revision = revision;
        }
        result
    }

    async fn props(&self, id: &str) -> Value {
        let read = self
            .s
            .call("get_subtree", json!({"projectId":self.p,"nodeId":id}))
            .await;
        read["nodes"][0]["props"].clone()
    }
}

#[tokio::test]
async fn bare_descendant_keys_on_set_merge_one_override() {
    let mut s = Session::seeded().await;
    let edited = s.apply("@set x t1=\"A\"").await;
    assert_eq!(edited["committedRevision"], 2, "{edited}");
    assert_eq!(
        s.props("x").await["overrides"],
        json!({"t1":{"text":"A"},"t2":{"fill":"#111111"}})
    );
    let edited = s.apply(r#"@set x t1={"text":"B","visible":false}"#).await;
    assert_eq!(edited["committedRevision"], 3, "{edited}");
    assert_eq!(
        s.props("x").await["overrides"],
        json!({"t1":{"text":"B","visible":false},"t2":{"fill":"#111111"}})
    );
}

#[tokio::test]
async fn misspelled_bare_keys_fail_with_a_suggestion() {
    let mut s = Session::seeded().await;
    // On an instance: the closest descendant or instance key.
    for (source, suggestion) in [("@set x t3=\"A\"", "t1"), ("@set x opacty=4", "opacity")] {
        let failed = s.apply(source).await;
        let error = &failed["error"];
        assert_eq!(
            error["field"],
            format!("props.overrides.{}", &source[7..source.find('=').unwrap()]),
            "{failed}"
        );
        assert_eq!(error["suggestion"], suggestion, "{failed}");
        assert_eq!(
            (&error["line"], &error["column"]),
            (&json!(1), &json!(1)),
            "{failed}"
        );
    }
    // On a node that is not an instance: descendant keys need an instance.
    let failed = s.apply("@set card opacty=4").await;
    let error = &failed["error"];
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("need an instance"), "{failed}");
    assert!(message.contains("did you mean `opacity`"), "{failed}");
    assert_eq!(error["suggestion"], "opacity", "{failed}");
    let failed = s.apply("@set card t1=\"A\"").await;
    let message = failed["error"]["message"].as_str().unwrap();
    assert!(message.contains("need an instance"), "{failed}");
    assert_eq!(s.revision, 1);
}

#[tokio::test]
async fn null_on_set_removes_props_fields_and_override_keys() {
    let mut s = Session::seeded().await;
    let edited = s
        .apply(
            "@set card maxWidth=null fill=null layout.gap=null\n@set x t2.fill=null t1.text=\"A\"",
        )
        .await;
    assert_eq!(edited["committedRevision"], 2, "{edited}");
    let card = s.props("card").await;
    assert!(
        card.get("maxWidth").is_none() && card.get("fill").is_none(),
        "{card}"
    );
    assert_eq!(card["layout"], json!({"mode":"column"}));
    assert_eq!(
        s.props("x").await["overrides"],
        json!({"t1":{"text":"A"},"t2":{}})
    );
    let edited = s.apply("@set x t2=null").await;
    assert_eq!(edited["committedRevision"], 3, "{edited}");
    assert_eq!(s.props("x").await["overrides"], json!({"t1":{"text":"A"}}));
}

#[tokio::test]
async fn bare_clip_and_hugging_roots_commit() {
    let mut s = Session::seeded().await;
    let source =
        "@in p\n  frame#a w=1440 h=900 clip\n  frame#b w=1440 h=900 clip=false\n@set b clip\n";
    let edited = s.apply(source).await;
    assert_eq!(edited["committedRevision"], 2, "{edited}");
    assert_eq!(s.props("a").await["clip"], true);
    assert_eq!(s.props("b").await["clip"], true);
    // A second page with a column root and no `h`: it hugs its content.
    let edited = s
        .apply("page#q\n  frame#r w=1440 col\n    frame#s w=10 h=50\n")
        .await;
    assert_eq!(edited["committedRevision"], 3, "{edited}");
    assert_eq!(s.props("r").await["height"], "hug");
    // A free root without `h` fails before storage, with the accepted forms.
    let failed = s.apply("page#f\n  frame#z w=1440 free\n").await;
    let error = &failed["error"];
    assert_eq!(error["code"], "SOURCE_PARSE_FAILED", "{failed}");
    assert_eq!(
        error["message"],
        "root frames need h=<n>, or h=hug with row/col"
    );
    assert_eq!(s.revision, 3);
}
