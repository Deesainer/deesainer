#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use deesainer_figfile::Value;

fn show(v: &Value, depth: usize, out: &mut String) {
    if out.len() > 3000 {
        return;
    }
    match v {
        Value::Object(f) => {
            out.push('{');
            for (k, x) in f {
                out.push_str(k);
                out.push(':');
                if depth < 6 {
                    show(x, depth + 1, out);
                } else {
                    out.push_str("..");
                }
                out.push(' ');
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push_str(&format!("[{}: ", a.len()));
            for x in a.iter().take(3) {
                show(x, depth + 1, out);
                out.push(' ');
            }
            out.push(']');
        }
        Value::Bytes(b) => out.push_str(&format!("<{} bytes>", b.len())),
        other => out.push_str(&format!("{other:?}")),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: figkeys FILE [FIELD...]");
    let fields: Vec<String> = args.collect();
    let f = deesainer_figfile::read(&std::fs::read(&path).expect("read")).expect("parse");
    let nodes = f
        .message
        .get("nodeChanges")
        .map_or(&[][..], Value::as_array);
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    for n in nodes {
        *types
            .entry(
                n.get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned(),
            )
            .or_default() += 1;
        if let Value::Object(fs) = n {
            for (k, _) in fs {
                *keys.entry(k.clone()).or_default() += 1;
            }
        }
    }
    if fields.is_empty() {
        println!("types {types:?}");
        if let Value::Object(top) = &f.message {
            println!(
                "message fields: {:?}",
                top.iter().map(|(k, _)| k).collect::<Vec<_>>()
            );
        }
        for (k, c) in keys {
            println!("{c:6} {k}");
        }
        return;
    }
    let limit: usize = std::env::var("FIGKEYS_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let grep = std::env::var("FIGKEYS_GREP").unwrap_or_default();
    let mut shown = 0;
    for n in nodes {
        for k in &fields {
            if let Some(v) = n.get(k) {
                let mut s = String::new();
                show(v, 0, &mut s);
                if !s.contains(&grep) {
                    continue;
                }
                println!(
                    "{} {:?} {k} = {s}",
                    n.get("type").and_then(Value::as_str).unwrap_or("?"),
                    n.get("name").and_then(Value::as_str).unwrap_or("")
                );
                shown += 1;
            }
        }
        if shown >= limit {
            break;
        }
    }
}
