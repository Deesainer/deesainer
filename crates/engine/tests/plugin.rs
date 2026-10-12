#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use deesainer_engine::Session;
use serde_json::{json, Value};

const GRID: &str = include_str!("../../../examples/plugins/grid/plugin.wat");

#[test]
fn grid_plugin_draws_twelve_cards_in_one_undo_step() {
    let mut s = Session::new().unwrap();
    let report = s.dispatch("plugin.run", &json!({"wat": GRID})).unwrap();
    assert_eq!(report["calls"], 12);
    assert_eq!(report["logs"][0], "Grid plugin done");
    let page = s.current_page();
    let kids = s.doc().get(page).unwrap().children.clone();
    assert_eq!(kids.len(), 12);
    let last = s.doc().get(*kids.last().unwrap()).unwrap();
    assert_eq!((last.rect.x, last.rect.y), (420.0, 200.0));
    s.dispatch("edit.undo", &Value::Null).unwrap();
    assert!(s.doc().get(page).unwrap().children.is_empty());
}

#[test]
fn plugins_cannot_recurse_or_escape() {
    let mut s = Session::new().unwrap();
    let recurse = r#"(module
        (import "deesainer" "call" (func $call (param i32 i32 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "plugin.run")
        (func (export "run") (if (i32.ge_s (call $call (i32.const 0) (i32.const 10) (i32.const 0) (i32.const 0)) (i32.const 0)) (then unreachable))))"#;
    let report = s.dispatch("plugin.run", &json!({"wat": recurse})).unwrap();
    assert_eq!(report["calls"], 1);
    let fs = r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))) (memory (export "memory") 1) (func (export "run")))"#;
    assert!(s.dispatch("plugin.run", &json!({"wat": fs})).is_err());
}
