use std::collections::BTreeMap;
use std::sync::Arc;

use deesainer_doc::{
    AssetId, CollectionId, Comment, CommentId, DocError, Document, Modes, Node, NodeId, PropKey,
    PropValue, Style, StyleId, Variable, VariableId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OpError {
    #[error(transparent)]
    Doc(#[from] DocError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Op {
    CreateNode {
        parent: NodeId,
        index: usize,
        node: Box<Node>,
    },
    DeleteNode {
        id: NodeId,
    },
    MoveNode {
        id: NodeId,
        parent: NodeId,
        index: usize,
    },
    SetProp {
        id: NodeId,
        value: PropValue,
    },
    SetStyle {
        id: StyleId,
        style: Option<Style>,
    },
    SetVariable {
        id: VariableId,
        variable: Option<Variable>,
    },
    SetModes {
        modes: Modes,
    },
    RestoreModes {
        modes: Modes,
        variables: BTreeMap<VariableId, Variable>,
    },
    SetCollection {
        id: CollectionId,
        modes: Option<Modes>,
    },
    RestoreCollection {
        id: CollectionId,
        modes: Option<Modes>,
        variables: BTreeMap<VariableId, Variable>,
    },
    SetComment {
        id: CommentId,
        comment: Option<Comment>,
    },
    SetAsset {
        id: AssetId,
        #[serde(with = "bytes_b64")]
        data: Option<Arc<Vec<u8>>>,
    },
}

mod bytes_b64 {
    use std::sync::Arc;

    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => {
                s.serialize_some(&base64::engine::general_purpose::STANDARD.encode(b.as_slice()))
            }
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Vec<u8>>>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        s.map(|s| {
            base64::engine::general_purpose::STANDARD
                .decode(s)
                .map(Arc::new)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpTarget {
    Structure(NodeId),
    Prop(NodeId, PropKey),
    Style(StyleId),
    Variable(VariableId),
    Modes,
    Collection(CollectionId),
    Asset(AssetId),
    Comment(CommentId),
}

impl Op {
    pub fn set(id: NodeId, value: PropValue) -> Op {
        Op::SetProp { id, value }
    }

    pub fn target(&self) -> OpTarget {
        match self {
            Op::CreateNode { node, .. } => OpTarget::Structure(node.id),
            Op::DeleteNode { id } | Op::MoveNode { id, .. } => OpTarget::Structure(*id),
            Op::SetProp { id, value } => OpTarget::Prop(*id, value.key()),
            Op::SetStyle { id, .. } => OpTarget::Style(*id),
            Op::SetVariable { id, .. } => OpTarget::Variable(*id),
            Op::SetModes { .. } | Op::RestoreModes { .. } => OpTarget::Modes,
            Op::SetCollection { id, .. } | Op::RestoreCollection { id, .. } => {
                OpTarget::Collection(*id)
            }
            Op::SetAsset { id, .. } => OpTarget::Asset(id.clone()),
            Op::SetComment { id, .. } => OpTarget::Comment(*id),
        }
    }

    pub fn apply(self, doc: &mut Document) -> Result<Op, OpError> {
        Ok(match self {
            Op::CreateNode {
                parent,
                index,
                node,
            } => {
                let id = node.id;
                doc.insert(*node, parent, index)?;
                Op::DeleteNode { id }
            }
            Op::DeleteNode { id } => {
                let (node, parent, index) = doc.remove_leaf(id)?;
                Op::CreateNode {
                    parent,
                    index,
                    node: Box::new(node),
                }
            }
            Op::MoveNode { id, parent, index } => {
                let (parent, index) = doc.move_node(id, parent, index)?;
                Op::MoveNode { id, parent, index }
            }
            Op::SetProp { id, value } => {
                let old = doc.set_prop(id, value)?;
                Op::SetProp { id, value: old }
            }
            Op::SetStyle { id, style } => {
                let old = doc.set_style(id, style);
                Op::SetStyle { id, style: old }
            }
            Op::SetVariable { id, variable } => {
                let old = doc.set_variable(id, variable)?;
                Op::SetVariable { id, variable: old }
            }
            Op::SetModes { modes } => {
                let (modes, variables) = doc.set_modes(modes)?;
                Op::RestoreModes { modes, variables }
            }
            Op::RestoreModes { modes, variables } => {
                let current_modes = doc.modes().clone();
                let current_vars: BTreeMap<VariableId, Variable> =
                    doc.variables().map(|(k, v)| (k, v.clone())).collect();
                doc.restore_modes(modes, variables);
                Op::RestoreModes {
                    modes: current_modes,
                    variables: current_vars,
                }
            }
            Op::SetCollection { id, modes } => {
                let (modes, variables) = doc.set_collection(id, modes)?;
                Op::RestoreCollection {
                    id,
                    modes,
                    variables,
                }
            }
            Op::RestoreCollection {
                id,
                modes,
                variables,
            } => {
                let current = doc.collection(id).cloned().filter(|_| !id.is_default());
                let current_vars = doc.collection_variables(id);
                doc.restore_collection(id, modes, variables);
                Op::RestoreCollection {
                    id,
                    modes: current,
                    variables: current_vars,
                }
            }
            Op::SetAsset { id, data } => {
                let old = doc.set_asset(id.clone(), data);
                Op::SetAsset { id, data: old }
            }
            Op::SetComment { id, comment } => {
                let old = doc.set_comment(id, comment);
                Op::SetComment { id, comment: old }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub label: String,
    pub ops: Vec<Op>,
}

impl Transaction {
    pub fn new(label: impl Into<String>, ops: Vec<Op>) -> Self {
        Self {
            label: label.into(),
            ops,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn apply(&self, doc: &mut Document) -> Result<Transaction, OpError> {
        let mut inverse = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            match op.clone().apply(doc) {
                Ok(inv) => inverse.push(inv),
                Err(err) => {
                    for inv in inverse.into_iter().rev() {
                        let _ = inv.apply(doc);
                    }
                    return Err(err);
                }
            }
        }
        inverse.reverse();
        Ok(Transaction::new(self.label.clone(), inverse))
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    group: Option<(Transaction, usize)>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commit(&mut self, doc: &mut Document, tx: &Transaction) -> Result<(), OpError> {
        let inverse = tx.apply(doc)?;
        self.push_inverse(inverse);
        Ok(())
    }

    fn push_inverse(&mut self, inverse: Transaction) {
        if inverse.is_empty() {
            return;
        }
        self.redo.clear();
        match &mut self.group {
            Some((group, _)) => {
                let mut ops = inverse.ops;
                ops.append(&mut group.ops);
                group.ops = ops;
            }
            None => self.undo.push(inverse),
        }
    }

    pub fn amend(&mut self, doc: &mut Document, ops: Vec<Op>) -> Result<(), OpError> {
        if ops.is_empty() {
            return Ok(());
        }
        let inverse = Transaction::new("", ops).apply(doc)?;
        let target = match &mut self.group {
            Some((group, _)) => Some(group),
            None => self.undo.last_mut(),
        };
        match target {
            Some(step) => {
                let mut new_ops = inverse.ops;
                new_ops.append(&mut step.ops);
                step.ops = new_ops;
            }
            None => self.undo.push(inverse),
        }
        Ok(())
    }

    pub fn begin_group(&mut self, label: impl Into<String>) {
        match &mut self.group {
            Some((_, depth)) => *depth += 1,
            None => self.group = Some((Transaction::new(label, Vec::new()), 1)),
        }
    }

    pub fn end_group(&mut self) {
        let Some((group, depth)) = self.group.take() else {
            return;
        };
        if depth > 1 {
            self.group = Some((group, depth - 1));
        } else if !group.is_empty() {
            self.undo.push(group);
        }
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<Option<Transaction>, OpError> {
        self.end_all_groups();
        Self::step(&mut self.undo, &mut self.redo, doc)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<Option<Transaction>, OpError> {
        self.end_all_groups();
        Self::step(&mut self.redo, &mut self.undo, doc)
    }

    fn end_all_groups(&mut self) {
        while self.group.is_some() {
            self.end_group();
        }
    }

    fn step(
        from: &mut Vec<Transaction>,
        to: &mut Vec<Transaction>,
        doc: &mut Document,
    ) -> Result<Option<Transaction>, OpError> {
        let Some(tx) = from.pop() else {
            return Ok(None);
        };
        match tx.apply(doc) {
            Ok(inverse) => {
                to.push(inverse);
                Ok(Some(tx))
            }
            Err(err) => {
                from.push(tx);
                Err(err)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.group.as_ref().is_some_and(|(g, _)| !g.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.group = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::NodeKind;
    use deesainer_geom::Rect;

    fn create_rect(doc: &Document) -> (NodeId, Op) {
        let id = doc.next_free_id();
        let node = Node::new(
            id,
            "Rectangle",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let op = Op::CreateNode {
            parent: doc.first_page(),
            index: 0,
            node: Box::new(node),
        };
        (id, op)
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let original = doc.clone();
        let mut history = History::new();
        let (id, create) = create_rect(&doc);
        let tx = Transaction::new(
            "Create rectangle",
            vec![
                create,
                Op::set(id, PropValue::Rect(Rect::new(5.0, 5.0, 20.0, 20.0))),
            ],
        );
        history.commit(&mut doc, &tx).unwrap();
        let edited = doc.clone();
        assert_eq!(
            history.undo(&mut doc).unwrap().map(|t| t.label).as_deref(),
            Some("Create rectangle")
        );
        assert_eq!(doc.get(page), original.get(page));
        assert!(doc.get(id).is_none());
        history.redo(&mut doc).unwrap();
        assert_eq!(doc, edited);
    }

    #[test]
    fn failed_transaction_leaves_document_unchanged() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let before = doc.clone();
        let (_, create) = create_rect(&doc);
        let tx = Transaction::new("Broken", vec![create, Op::DeleteNode { id: NodeId(9999) }]);
        assert!(tx.apply(&mut doc).is_err());
        assert_eq!(doc.get(page), before.get(page));
        assert_eq!(doc.len(), before.len());
    }

    #[test]
    fn groups_collapse_into_one_step() {
        let mut doc = Document::new();
        let mut history = History::new();
        let (id, create) = create_rect(&doc);
        history
            .commit(&mut doc, &Transaction::new("Create", vec![create]))
            .unwrap();
        let after_create = doc.clone();
        history.begin_group("Drag");
        for i in 1..=5 {
            let rect = Rect::new(f64::from(i), 0.0, 10.0, 10.0);
            history
                .commit(
                    &mut doc,
                    &Transaction::new("Move", vec![Op::set(id, PropValue::Rect(rect))]),
                )
                .unwrap();
        }
        history.end_group();
        assert_eq!(
            history.undo(&mut doc).unwrap().map(|t| t.label).as_deref(),
            Some("Drag")
        );
        assert_eq!(doc, after_create);
    }

    #[test]
    fn amend_joins_the_last_step() {
        let mut doc = Document::new();
        let original = doc.clone();
        let mut history = History::new();
        let (id, create) = create_rect(&doc);
        history
            .commit(&mut doc, &Transaction::new("Create", vec![create]))
            .unwrap();
        history
            .amend(
                &mut doc,
                vec![Op::set(id, PropValue::Name("Derived".into()))],
            )
            .unwrap();
        history.undo(&mut doc).unwrap();
        assert_eq!(
            doc.get(doc.first_page()),
            original.get(original.first_page())
        );
        assert!(!history.can_undo());
    }

    #[test]
    fn modes_round_trip_through_ops() {
        let mut doc = Document::new();
        let before = doc.clone();
        let inverse = Op::SetModes {
            modes: Modes {
                names: vec!["Light".into(), "Dark".into()],
                active: 0,
                ..Modes::default()
            },
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(doc.modes().names.len(), 2);
        inverse.apply(&mut doc).unwrap();
        assert_eq!(doc, before);
    }

    #[test]
    fn empty_stacks_return_none() {
        let mut doc = Document::new();
        let mut history = History::new();
        assert!(history.undo(&mut doc).unwrap().is_none());
        assert!(history.redo(&mut doc).unwrap().is_none());
    }
}
