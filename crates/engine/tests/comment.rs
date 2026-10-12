#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.dispatch(cmd, &params)
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

#[test]
fn comment_threads() {
    let mut s = Session::new().unwrap();
    s.set_user("Ada");
    s.set_clock(|| "2026-10-09T10:00:00Z".to_owned());
    let id = run(
        &mut s,
        "comment.add",
        json!({"x": 10, "y": 20, "text": "Make this bigger"}),
    )["id"]
        .as_u64()
        .unwrap();
    s.set_user("Bob");
    run(&mut s, "comment.reply", json!({"id": id, "text": "Done"}));
    run(&mut s, "comment.resolve", json!({"id": id}));
    let list = run(&mut s, "comment.list", Value::Null);
    assert_eq!(list[0]["author"], "Ada");
    assert_eq!(list[0]["created"], "2026-10-09T10:00:00Z");
    assert_eq!(list[0]["replies"][0]["author"], "Bob");
    assert_eq!(list[0]["resolved"], true);
    assert!(s
        .dispatch("comment.add", &json!({"x": 0, "y": 0, "text": "  "}))
        .is_err());
    let data = run(&mut s, "document.serialize", Value::Null)["data"].clone();
    let mut other = Session::new().unwrap();
    run(&mut other, "document.load", json!({"data": data}));
    assert_eq!(
        run(&mut other, "comment.list", Value::Null)
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(&mut s, "comment.delete", json!({"id": id}));
    assert!(run(&mut s, "comment.list", Value::Null)
        .as_array()
        .unwrap()
        .is_empty());
}
