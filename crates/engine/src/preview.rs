use std::collections::BTreeMap;

use deesainer_doc::{Document, VarValue, VariableId};
use deesainer_ops::Transaction;

pub fn with_variable_values(doc: &Document, values: &BTreeMap<VariableId, VarValue>) -> Document {
    let mut out = doc.clone();
    if values.is_empty() {
        return out;
    }
    for (id, value) in values {
        let Some(mut v) = out.variable(*id).cloned() else {
            continue;
        };
        let fits = out
            .resolve_variable(*id)
            .is_some_and(|cur| cur.type_name() == value.type_name());
        if !fits {
            continue;
        }
        for slot in &mut v.values {
            *slot = value.clone();
        }
        let _ = out.set_variable(*id, Some(v));
    }
    for pass in crate::derive::PASSES {
        let ops = pass(&out);
        if !ops.is_empty() {
            let _ = Transaction::new("preview", ops).apply(&mut out);
        }
    }
    out
}
