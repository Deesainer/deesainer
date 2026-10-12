mod comment;
mod ids;
mod layout;
mod network;
mod node;
mod paint;
mod proto;
mod style;
mod text;
mod vector;

use std::collections::BTreeMap;
use std::sync::Arc;

use deesainer_color::Rgba;
use deesainer_geom::{Affine, Point, Rect};
use serde::{Deserialize, Serialize};

pub use comment::{Comment, Reply};
pub use ids::{AssetId, CollectionId, CommentId, NodeId, StyleId, VariableId};
pub use layout::{
    AutoLayout, Constraint, Constraints, CounterAlign, ExportFormat, ExportSetting, GridAlign,
    GridPattern, LayoutChild, LayoutDirection, LayoutGrid, Padding, PrimaryAlign, RulerGuide,
    SizeLimits, Sizing,
};
pub use network::{RegionEdge, RegionPaint, Segment, VectorNetwork};
pub use node::{
    Annotation, BoolOp, ComponentProp, ComponentPropKind, CornerRadius, LibraryRef, Node, NodeKind,
    Override, PropKey, PropValue,
};
pub use paint::{
    mirror_paints, ColorStop, Effect, ImageAdjust, ImageScaleMode, Paint, PaintKind, StrokeAlign,
    StrokeCap, StrokeJoin, StrokeStyle,
};
pub use proto::{
    Action, CompareOp, Condition, Device, Direction, Easing, Interaction, Overflow, Scrolling,
    Transition, TransitionKind, Trigger,
};
pub use style::{
    BindTarget, Bindings, LibraryLink, Modes, Style, StyleKind, StyleRefs, VarValue, Variable,
};
pub use text::{
    FontFeature, FontVariation, LineHeight, ParagraphFormat, TextAlignH, TextAlignV,
    TextAutoResize, TextCase, TextData, TextDecoration, TextList, TextRun, TextStyle,
};
pub use vector::{FillRule, PathCmd, VectorPath};

pub const MAX_DEPTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    #[error("no node with id {0}")]
    UnknownNode(NodeId),
    #[error("a node with id {0} already exists")]
    DuplicateNode(NodeId),
    #[error("node {0} cannot contain children")]
    NotAContainer(NodeId),
    #[error("index {index} is out of range for node {parent} ({len} children)")]
    IndexOutOfRange {
        parent: NodeId,
        index: usize,
        len: usize,
    },
    #[error("the document root cannot be removed or moved")]
    CannotRemoveRoot,
    #[error("node {0} still has children; remove them first")]
    HasChildren(NodeId),
    #[error("node {0} must be inserted without children")]
    InsertWithChildren(NodeId),
    #[error("pages can only live directly under the document, and only pages can")]
    PagePlacement,
    #[error("the last page cannot be removed")]
    LastPage,
    #[error("moving {0} into {1} would create a cycle")]
    Cycle(NodeId, NodeId),
    #[error("document is deeper than {MAX_DEPTH} levels or cyclic")]
    TooDeep,
    #[error("changing the kind of {0} is not allowed here")]
    KindChange(NodeId),
    #[error("variable {0} needs one value per mode")]
    ModeMismatch(VariableId),
    #[error("invalid document: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    root: NodeId,
    next_id: u64,
    nodes: BTreeMap<NodeId, Node>,
    #[serde(default)]
    styles: BTreeMap<StyleId, Style>,
    #[serde(default)]
    variables: BTreeMap<VariableId, Variable>,
    #[serde(default)]
    modes: Modes,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    collections: BTreeMap<CollectionId, Modes>,
    #[serde(default)]
    comments: BTreeMap<CommentId, Comment>,
    #[serde(skip)]
    assets: BTreeMap<AssetId, Arc<Vec<u8>>>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        let root = NodeId(1);
        let page_id = NodeId(2);
        let mut doc_node = Node::new(root, "Document", NodeKind::Document, Rect::default());
        doc_node.children.push(page_id);
        let mut page = Node::new(page_id, "Page 1", NodeKind::Page, Rect::default());
        page.parent = Some(root);
        page.fills
            .push(Paint::solid(Rgba::from_rgba8([0xF5, 0xF5, 0xF5, 0xFF])));
        Self {
            root,
            next_id: 3,
            nodes: BTreeMap::from([(root, doc_node), (page_id, page)]),
            styles: BTreeMap::new(),
            variables: BTreeMap::new(),
            modes: Modes::default(),
            collections: BTreeMap::new(),
            comments: BTreeMap::new(),
            assets: BTreeMap::new(),
        }
    }

    pub fn with_frame(width: f64, height: f64, background: Rgba) -> Result<Self, DocError> {
        let mut doc = Self::new();
        let mut frame = Node::new(
            doc.next_free_id(),
            "Frame 1",
            NodeKind::Frame,
            Rect::new(0.0, 0.0, width, height),
        );
        frame.fills.push(Paint::solid(background));
        let page = doc.first_page();
        doc.insert(frame, page, 0)?;
        Ok(doc)
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn pages(&self) -> &[NodeId] {
        self.get(self.root)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn first_page(&self) -> NodeId {
        self.pages().first().copied().unwrap_or(self.root)
    }

    pub fn next_free_id(&self) -> NodeId {
        NodeId(self.next_id)
    }

    pub fn fresh_ids(&self, n: usize) -> Vec<NodeId> {
        (0..n as u64)
            .map(|i| NodeId(self.next_id.saturating_add(i)))
            .collect()
    }

    pub fn reserve_ids(&mut self, raw: u64) {
        self.next_id = self.next_id.max(raw);
    }

    fn bump(&mut self, raw: u64) {
        self.next_id = self.next_id.max(raw.saturating_add(1));
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn node(&self, id: NodeId) -> Result<&Node, DocError> {
        self.nodes.get(&id).ok_or(DocError::UnknownNode(id))
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    fn check_placement(parent: &Node, child_kind: &NodeKind) -> Result<(), DocError> {
        let child_is_page = matches!(child_kind, NodeKind::Page);
        let parent_is_doc = matches!(parent.kind, NodeKind::Document);
        if child_is_page != parent_is_doc || matches!(child_kind, NodeKind::Document) {
            return Err(DocError::PagePlacement);
        }
        let into_instance = matches!(parent.kind, NodeKind::Instance { .. });
        let is_slot = matches!(child_kind, NodeKind::SlotContent { .. });
        if into_instance || is_slot {
            return if into_instance && is_slot {
                Ok(())
            } else {
                Err(DocError::NotAContainer(parent.id))
            };
        }
        if !parent.kind.is_container() {
            return Err(DocError::NotAContainer(parent.id));
        }
        Ok(())
    }

    pub fn insert(&mut self, mut node: Node, parent: NodeId, index: usize) -> Result<(), DocError> {
        if self.nodes.contains_key(&node.id) {
            return Err(DocError::DuplicateNode(node.id));
        }
        if !node.children.is_empty() {
            return Err(DocError::InsertWithChildren(node.id));
        }
        let p = self.node(parent)?;
        Self::check_placement(p, &node.kind)?;
        if index > p.children.len() {
            return Err(DocError::IndexOutOfRange {
                parent,
                index,
                len: p.children.len(),
            });
        }
        let p = self
            .nodes
            .get_mut(&parent)
            .ok_or(DocError::UnknownNode(parent))?;
        p.children.insert(index, node.id);
        node.parent = Some(parent);
        self.bump(node.id.0);
        self.nodes.insert(node.id, node);
        Ok(())
    }

    pub fn remove_leaf(&mut self, id: NodeId) -> Result<(Node, NodeId, usize), DocError> {
        if id == self.root {
            return Err(DocError::CannotRemoveRoot);
        }
        let node = self.node(id)?;
        if !node.children.is_empty() {
            return Err(DocError::HasChildren(id));
        }
        if matches!(node.kind, NodeKind::Page) && self.pages().len() <= 1 {
            return Err(DocError::LastPage);
        }
        let parent = node.parent.ok_or(DocError::UnknownNode(id))?;
        let index = self.index_in_parent(id)?;
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.remove(index);
        }
        let node = self.nodes.remove(&id).ok_or(DocError::UnknownNode(id))?;
        Ok((node, parent, index))
    }

    pub fn index_in_parent(&self, id: NodeId) -> Result<usize, DocError> {
        let parent = self.node(id)?.parent.ok_or(DocError::CannotRemoveRoot)?;
        self.node(parent)?
            .children
            .iter()
            .position(|c| *c == id)
            .ok_or(DocError::UnknownNode(id))
    }

    pub fn move_node(
        &mut self,
        id: NodeId,
        parent: NodeId,
        index: usize,
    ) -> Result<(NodeId, usize), DocError> {
        if id == self.root {
            return Err(DocError::CannotRemoveRoot);
        }
        let node = self.node(id)?;
        let old_parent = node.parent.ok_or(DocError::CannotRemoveRoot)?;
        let target = self.node(parent)?;
        Self::check_placement(target, &node.kind)?;
        if id == parent || self.is_ancestor(id, parent)? {
            return Err(DocError::Cycle(id, parent));
        }
        let old_index = self.index_in_parent(id)?;
        let len_after_removal = if old_parent == parent {
            target.children.len().saturating_sub(1)
        } else {
            target.children.len()
        };
        if index > len_after_removal {
            return Err(DocError::IndexOutOfRange {
                parent,
                index,
                len: len_after_removal,
            });
        }
        if let Some(p) = self.nodes.get_mut(&old_parent) {
            p.children.remove(old_index);
        }
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.insert(index, id);
        }
        if let Some(n) = self.nodes.get_mut(&id) {
            n.parent = Some(parent);
        }
        Ok((old_parent, old_index))
    }

    pub fn set_prop(&mut self, id: NodeId, value: PropValue) -> Result<PropValue, DocError> {
        if let PropValue::Kind(kind) = &value {
            let node = self.nodes.get(&id).ok_or(DocError::UnknownNode(id))?;
            let structural = |k: &NodeKind| matches!(k, NodeKind::Page | NodeKind::Document);
            let slots_only = node.children.iter().all(|c| {
                matches!(
                    self.nodes.get(c).map(|n| &n.kind),
                    Some(NodeKind::SlotContent { .. })
                )
            });
            let children_ok = node.children.is_empty()
                || match kind {
                    NodeKind::Instance { .. } => slots_only,
                    k => k.is_container() && !matches!(node.kind, NodeKind::Instance { .. }),
                };
            if structural(&node.kind) || structural(kind) || !children_ok {
                return Err(DocError::KindChange(id));
            }
        }
        let node = self.nodes.get_mut(&id).ok_or(DocError::UnknownNode(id))?;
        Ok(node.set(value))
    }

    pub fn is_ancestor(&self, ancestor: NodeId, id: NodeId) -> Result<bool, DocError> {
        let mut current = self.node(id)?.parent;
        let mut depth = 0;
        while let Some(p) = current {
            if p == ancestor {
                return Ok(true);
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(DocError::TooDeep);
            }
            current = self.node(p)?.parent;
        }
        Ok(false)
    }

    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut current = self.get(id).and_then(|n| n.parent);
        while let Some(p) = current {
            if out.len() > MAX_DEPTH {
                break;
            }
            out.push(p);
            current = self.get(p).and_then(|n| n.parent);
        }
        out
    }

    pub fn page_of(&self, id: NodeId) -> Option<NodeId> {
        if matches!(self.get(id)?.kind, NodeKind::Page) {
            return Some(id);
        }
        self.ancestors(id)
            .into_iter()
            .find(|a| matches!(self.get(*a).map(|n| &n.kind), Some(NodeKind::Page)))
    }

    pub fn subtree_post_order(&self, id: NodeId) -> Result<Vec<NodeId>, DocError> {
        let mut out = self.subtree_pre_order(id)?;
        out.reverse();
        Ok(out)
    }

    pub fn subtree_pre_order(&self, id: NodeId) -> Result<Vec<NodeId>, DocError> {
        let mut out = Vec::new();
        let mut stack = vec![(id, 0usize)];
        while let Some((current, depth)) = stack.pop() {
            if depth > MAX_DEPTH || out.len() > self.nodes.len() {
                return Err(DocError::TooDeep);
            }
            let node = self.node(current)?;
            out.push(current);
            for child in node.children.iter().rev() {
                stack.push((*child, depth + 1));
            }
        }
        Ok(out)
    }

    pub fn local_transform(node: &Node) -> Affine {
        if matches!(node.kind, NodeKind::Document | NodeKind::Page) {
            return Affine::IDENTITY;
        }
        let t = Affine::translate(node.rect.x, node.rect.y);
        if node.rotation == 0.0 {
            t
        } else {
            t.then(Affine::rotate_about(
                node.rotation,
                Point::new(node.rect.width / 2.0, node.rect.height / 2.0),
            ))
        }
    }

    pub fn slot_target(&self, instance: NodeId, name: &str) -> Option<NodeId> {
        let NodeKind::Instance { component, .. } = &self.get(instance)?.kind else {
            return None;
        };
        let NodeKind::Component { properties, .. } = &self.get(*component)?.kind else {
            return None;
        };
        properties
            .iter()
            .find(|p| {
                p.name == name
                    && matches!(
                        p.kind,
                        crate::ComponentPropKind::Slot { .. }
                            | crate::ComponentPropKind::InstanceSwap { .. }
                    )
            })
            .map(|p| p.target)
    }

    pub fn slot_fills(&self, instance: NodeId) -> BTreeMap<NodeId, NodeId> {
        match self.get(instance).map(|n| &n.kind) {
            Some(NodeKind::Instance { component, .. }) => {
                self.slot_fills_from(instance, *component)
            }
            _ => BTreeMap::new(),
        }
    }

    pub fn slot_fills_from(&self, instance: NodeId, main: NodeId) -> BTreeMap<NodeId, NodeId> {
        let (Some(node), Some(NodeKind::Component { properties, .. })) =
            (self.get(instance), self.get(main).map(|m| &m.kind))
        else {
            return BTreeMap::new();
        };
        node.children
            .iter()
            .filter_map(|c| match &self.get(*c)?.kind {
                NodeKind::SlotContent { slot } => properties
                    .iter()
                    .find(|p| {
                        p.name == *slot
                            && matches!(
                                p.kind,
                                crate::ComponentPropKind::Slot { .. }
                                    | crate::ComponentPropKind::InstanceSwap { .. }
                            )
                    })
                    .map(|p| (p.target, *c)),
                _ => None,
            })
            .collect()
    }

    pub fn world_transform(&self, id: NodeId) -> Affine {
        let mut chain = vec![id];
        chain.extend(self.ancestors(id));
        chain
            .iter()
            .rev()
            .filter_map(|n| self.get(*n))
            .fold(Affine::IDENTITY, |acc, n| {
                acc.then(Self::local_transform(n))
            })
    }

    pub fn world_bounds(&self, id: NodeId) -> Option<Rect> {
        let node = self.get(id)?;
        let local = Rect::new(0.0, 0.0, node.rect.width, node.rect.height);
        Some(self.world_transform(id).transform_rect_bounds(&local))
    }

    pub fn find_by_name(&self, name: &str) -> Option<NodeId> {
        self.nodes.values().find(|n| n.name == name).map(|n| n.id)
    }

    pub fn style(&self, id: StyleId) -> Option<&Style> {
        self.styles.get(&id)
    }

    pub fn styles(&self) -> impl Iterator<Item = (StyleId, &Style)> {
        self.styles.iter().map(|(k, v)| (*k, v))
    }

    pub fn set_style(&mut self, id: StyleId, style: Option<Style>) -> Option<Style> {
        self.bump(id.0);
        match style {
            Some(s) => self.styles.insert(id, s),
            None => self.styles.remove(&id),
        }
    }

    pub fn modes(&self) -> &Modes {
        &self.modes
    }

    pub fn variable(&self, id: VariableId) -> Option<&Variable> {
        self.variables.get(&id)
    }

    pub fn variables(&self) -> impl Iterator<Item = (VariableId, &Variable)> {
        self.variables.iter().map(|(k, v)| (*k, v))
    }

    pub fn set_variable(
        &mut self,
        id: VariableId,
        variable: Option<Variable>,
    ) -> Result<Option<Variable>, DocError> {
        if let Some(v) = &variable {
            let fits = self
                .collection(v.collection)
                .is_some_and(|c| c.names.len() == v.values.len());
            if !fits {
                return Err(DocError::ModeMismatch(id));
            }
        }
        self.bump(id.0);
        Ok(match variable {
            Some(v) => self.variables.insert(id, v),
            None => self.variables.remove(&id),
        })
    }

    pub fn set_modes(
        &mut self,
        modes: Modes,
    ) -> Result<(Modes, BTreeMap<VariableId, Variable>), DocError> {
        if modes.names.is_empty() || modes.active >= modes.names.len() {
            return Err(DocError::Invalid(
                "modes need at least one name and a valid active index".into(),
            ));
        }
        let old_vars = self.variables.clone();
        let n = modes.names.len();
        for v in self
            .variables
            .values_mut()
            .filter(|v| v.collection.is_default())
        {
            let fill = v.values.first().cloned();
            if let Some(fill) = fill {
                v.values.resize(n, fill);
            }
        }
        Ok((std::mem::replace(&mut self.modes, modes), old_vars))
    }

    pub fn collection(&self, id: CollectionId) -> Option<&Modes> {
        if id.is_default() {
            Some(&self.modes)
        } else {
            self.collections.get(&id)
        }
    }

    pub fn collections(&self) -> impl Iterator<Item = (CollectionId, &Modes)> {
        std::iter::once((CollectionId::DEFAULT, &self.modes))
            .chain(self.collections.iter().map(|(k, v)| (*k, v)))
    }

    pub fn collection_variables(&self, id: CollectionId) -> BTreeMap<VariableId, Variable> {
        self.variables
            .iter()
            .filter(|(_, v)| v.collection == id)
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    #[allow(clippy::type_complexity)]
    pub fn set_collection(
        &mut self,
        id: CollectionId,
        modes: Option<Modes>,
    ) -> Result<(Option<Modes>, BTreeMap<VariableId, Variable>), DocError> {
        if id.is_default() {
            return Err(DocError::Invalid(
                "the default collection is changed through its modes".into(),
            ));
        }
        if let Some(m) = &modes {
            if m.names.is_empty() || m.active >= m.names.len() {
                return Err(DocError::Invalid(
                    "modes need at least one name and a valid active index".into(),
                ));
            }
        }
        let old_vars = self.collection_variables(id);
        self.bump(id.0);
        let old = match modes {
            Some(m) => {
                let n = m.names.len();
                for v in self.variables.values_mut().filter(|v| v.collection == id) {
                    if let Some(fill) = v.values.first().cloned() {
                        v.values.resize(n, fill);
                    }
                }
                self.collections.insert(id, m)
            }
            None => {
                self.variables.retain(|_, v| v.collection != id);
                self.collections.remove(&id)
            }
        };
        Ok((old, old_vars))
    }

    pub fn restore_collection(
        &mut self,
        id: CollectionId,
        modes: Option<Modes>,
        variables: BTreeMap<VariableId, Variable>,
    ) {
        if id.is_default() {
            return;
        }
        match modes {
            Some(m) => {
                self.collections.insert(id, m);
            }
            None => {
                self.collections.remove(&id);
            }
        }
        self.variables.retain(|_, v| v.collection != id);
        self.variables.extend(variables);
    }

    pub fn mode_at(&self, node: NodeId, collection: CollectionId) -> Option<usize> {
        let modes = self.collection(collection)?;
        let mut at = Some(node);
        for _ in 0..MAX_DEPTH {
            let Some(n) = at.and_then(|id| self.nodes.get(&id)) else {
                break;
            };
            if let Some(m) = n.variable_modes.get(&collection) {
                if *m < modes.names.len() {
                    return Some(*m);
                }
            }
            at = n.parent;
        }
        Some(modes.active)
    }

    pub fn resolve_variable_at(&self, id: VariableId, node: NodeId) -> Option<&VarValue> {
        let mut current = id;
        for _ in 0..32 {
            let v = self.variables.get(&current)?;
            let mode = self.mode_at(node, v.collection)?;
            match v.values.get(mode)? {
                VarValue::Alias(next) => current = *next,
                other => return Some(other),
            }
        }
        None
    }

    pub fn restore_modes(&mut self, modes: Modes, variables: BTreeMap<VariableId, Variable>) {
        self.modes = modes;
        self.variables = variables;
    }

    pub fn resolve_variable(&self, id: VariableId) -> Option<&VarValue> {
        let mut current = id;
        for _ in 0..32 {
            let v = self.variables.get(&current)?;
            let mode = self.collection(v.collection)?.active;
            match v.values.get(mode)? {
                VarValue::Alias(next) => current = *next,
                other => return Some(other),
            }
        }
        None
    }

    pub fn comment(&self, id: CommentId) -> Option<&Comment> {
        self.comments.get(&id)
    }

    pub fn comments(&self) -> impl Iterator<Item = (CommentId, &Comment)> {
        self.comments.iter().map(|(k, v)| (*k, v))
    }

    pub fn set_comment(&mut self, id: CommentId, comment: Option<Comment>) -> Option<Comment> {
        self.bump(id.0);
        match comment {
            Some(c) => self.comments.insert(id, c),
            None => self.comments.remove(&id),
        }
    }

    pub fn asset(&self, id: &AssetId) -> Option<&Arc<Vec<u8>>> {
        self.assets.get(id)
    }

    pub fn assets(&self) -> impl Iterator<Item = (&AssetId, &Arc<Vec<u8>>)> {
        self.assets.iter()
    }

    pub fn insert_asset(&mut self, id: AssetId, bytes: Arc<Vec<u8>>) {
        self.assets.insert(id, bytes);
    }

    pub fn set_asset(&mut self, id: AssetId, bytes: Option<Arc<Vec<u8>>>) -> Option<Arc<Vec<u8>>> {
        match bytes {
            Some(b) => self.assets.insert(id, b),
            None => self.assets.remove(&id),
        }
    }

    pub fn validate(&self) -> Result<(), DocError> {
        let invalid = |m: String| Err(DocError::Invalid(m));
        let root = self.node(self.root)?;
        if !matches!(root.kind, NodeKind::Document) || root.parent.is_some() {
            return invalid("root must be a parentless document node".into());
        }
        if self.pages().is_empty() {
            return invalid("a document needs at least one page".into());
        }
        let reachable = self.subtree_pre_order(self.root)?;
        if reachable.len() != self.nodes.len() {
            return invalid("unreachable or duplicated nodes".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &reachable {
            if !seen.insert(*id) {
                return invalid(format!("node {id} appears twice"));
            }
            let node = self.node(*id)?;
            if let NodeKind::Text(data) = &node.kind {
                data.validate_runs().map_err(DocError::Invalid)?;
            }
            if node.id != *id {
                return invalid(format!("node key {id} holds id {}", node.id));
            }
            if id.0 >= self.next_id {
                return invalid(format!("node {id} is not below the id counter"));
            }
            for child in &node.children {
                let c = self.node(*child)?;
                if c.parent != Some(*id) {
                    return invalid(format!("node {child} has the wrong parent"));
                }
                Self::check_placement(node, &c.kind)?;
            }
        }
        for (id, v) in &self.variables {
            let fits = self
                .collection(v.collection)
                .is_some_and(|c| c.names.len() == v.values.len());
            if !fits {
                return Err(DocError::ModeMismatch(*id));
            }
        }
        for (_, m) in self.collections() {
            if m.names.is_empty() || m.active >= m.names.len() {
                return invalid("active mode out of range".into());
            }
        }
        if self.collections.contains_key(&CollectionId::DEFAULT) {
            return invalid("collection 0 is the default collection".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_node(doc: &Document) -> Node {
        Node::new(
            doc.next_free_id(),
            "Rectangle",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )
    }

    fn first_frame(doc: &Document) -> NodeId {
        doc.get(doc.first_page()).unwrap().children[0]
    }

    #[test]
    fn new_document_is_valid() {
        let doc = Document::new();
        doc.validate().unwrap();
        assert_eq!(doc.pages().len(), 1);
    }

    #[test]
    fn insert_and_remove_restore_the_document() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let before = doc.clone();
        let node = rect_node(&doc);
        let id = node.id;
        doc.insert(node, page, 0).unwrap();
        doc.validate().unwrap();
        let (node, parent, index) = doc.remove_leaf(id).unwrap();
        assert_eq!((parent, index), (page, 0));
        assert_eq!(doc.get(page), before.get(page));
        assert_eq!(node.id, id);
        assert_ne!(doc.next_free_id(), id);
    }

    #[test]
    fn slot_content_lives_only_in_instances() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let main = Node::new(
            doc.next_free_id(),
            "Main",
            NodeKind::Component {
                description: String::new(),
                properties: Vec::new(),
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let main_id = main.id;
        doc.insert(main, page, 0).unwrap();
        let inst = Node::new(
            doc.next_free_id(),
            "Inst",
            NodeKind::Instance {
                component: main_id,
                overrides: Vec::new(),
            },
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        let inst_id = inst.id;
        doc.insert(inst, page, 1).unwrap();
        let slot = |doc: &Document| {
            Node::new(
                doc.next_free_id(),
                "S",
                NodeKind::SlotContent {
                    slot: "Body".into(),
                },
                Rect::default(),
            )
        };
        assert_eq!(
            doc.insert(slot(&doc), page, 0),
            Err(DocError::NotAContainer(page))
        );
        let s1 = slot(&doc);
        let s1_id = s1.id;
        doc.insert(s1, inst_id, 0).unwrap();
        assert_eq!(
            doc.insert(rect_node(&doc), inst_id, 0),
            Err(DocError::NotAContainer(inst_id))
        );
        doc.insert(rect_node(&doc), s1_id, 0).unwrap();
        assert_eq!(
            doc.set_prop(inst_id, PropValue::Kind(NodeKind::Frame)),
            Err(DocError::KindChange(inst_id))
        );
        assert!(doc
            .set_prop(
                inst_id,
                PropValue::Kind(NodeKind::Instance {
                    component: main_id,
                    overrides: Vec::new()
                })
            )
            .is_ok());
    }

    #[test]
    fn placement_rules() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let r = rect_node(&doc);
        let rid = r.id;
        doc.insert(r, page, 0).unwrap();
        assert_eq!(
            doc.insert(rect_node(&doc), rid, 0),
            Err(DocError::NotAContainer(rid))
        );
        assert_eq!(
            doc.insert(rect_node(&doc), doc.root(), 0),
            Err(DocError::PagePlacement)
        );
        let p2 = Node::new(doc.next_free_id(), "P", NodeKind::Page, Rect::default());
        assert_eq!(doc.insert(p2, page, 0), Err(DocError::PagePlacement));
        assert_eq!(
            doc.remove_leaf(page).map(|_| ()),
            Err(DocError::HasChildren(page))
        );
    }

    #[test]
    fn last_page_and_root_are_protected() {
        let mut doc = Document::new();
        let page = doc.first_page();
        assert_eq!(doc.remove_leaf(page).map(|_| ()), Err(DocError::LastPage));
        assert_eq!(
            doc.remove_leaf(doc.root()).map(|_| ()),
            Err(DocError::CannotRemoveRoot)
        );
    }

    #[test]
    fn move_rejects_cycles_and_round_trips() {
        let mut doc = Document::with_frame(100.0, 100.0, Rgba::WHITE).unwrap();
        let page = doc.first_page();
        let frame = first_frame(&doc);
        let r = rect_node(&doc);
        let rid = r.id;
        doc.insert(r, page, 1).unwrap();
        let before = doc.clone();
        let (op, oi) = doc.move_node(rid, frame, 0).unwrap();
        assert_eq!((op, oi), (page, 1));
        assert_eq!(doc.node(rid).unwrap().parent, Some(frame));
        doc.validate().unwrap();
        doc.move_node(rid, op, oi).unwrap();
        assert_eq!(doc, before);
        assert_eq!(
            doc.move_node(frame, frame, 0),
            Err(DocError::Cycle(frame, frame))
        );
    }

    #[test]
    fn world_transform_accumulates() {
        let mut doc = Document::with_frame(100.0, 100.0, Rgba::WHITE).unwrap();
        let frame = first_frame(&doc);
        doc.set_prop(frame, PropValue::Rect(Rect::new(50.0, 50.0, 100.0, 100.0)))
            .unwrap();
        let r = Node::new(
            doc.next_free_id(),
            "R",
            NodeKind::Rectangle,
            Rect::new(10.0, 10.0, 20.0, 20.0),
        );
        let rid = r.id;
        doc.insert(r, frame, 0).unwrap();
        assert_eq!(
            doc.world_bounds(rid).unwrap(),
            Rect::new(60.0, 60.0, 20.0, 20.0)
        );
    }

    #[test]
    fn set_prop_returns_old_and_guards_kind() {
        let mut doc = Document::with_frame(100.0, 100.0, Rgba::WHITE).unwrap();
        let page = doc.first_page();
        let frame = first_frame(&doc);
        let old = doc.set_prop(frame, PropValue::Name("Card".into())).unwrap();
        assert_eq!(old, PropValue::Name("Frame 1".into()));
        assert_eq!(
            doc.set_prop(page, PropValue::Kind(NodeKind::Rectangle)),
            Err(DocError::KindChange(page))
        );
    }

    #[test]
    fn variables_resolve_aliases_and_follow_modes() {
        let mut doc = Document::new();
        let a = VariableId(100);
        let b = VariableId(101);
        doc.set_variable(
            a,
            Some(Variable {
                name: "blue".into(),
                values: vec![VarValue::Color(Rgba::BLACK)],
                ..Default::default()
            }),
        )
        .unwrap();
        doc.set_variable(
            b,
            Some(Variable {
                name: "primary".into(),
                values: vec![VarValue::Alias(a)],
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(doc.resolve_variable(b), Some(&VarValue::Color(Rgba::BLACK)));
        assert!(doc.next_free_id().0 > 101);
        let bad = Variable {
            name: "x".into(),
            values: vec![],
            ..Default::default()
        };
        assert_eq!(
            doc.set_variable(VariableId(5), Some(bad)),
            Err(DocError::ModeMismatch(VariableId(5)))
        );
        let before = doc.clone();
        let (old_modes, old_vars) = doc
            .set_modes(Modes {
                names: vec!["Light".into(), "Dark".into()],
                active: 1,
                ..Modes::default()
            })
            .unwrap();
        doc.validate().unwrap();
        assert_eq!(doc.variable(a).unwrap().values.len(), 2);
        doc.restore_modes(old_modes, old_vars);
        assert_eq!(doc, before);
    }

    #[test]
    fn serde_round_trip() {
        let mut doc = Document::with_frame(1440.0, 1024.0, Rgba::WHITE).unwrap();
        let page = doc.first_page();
        let mut t = Node::new(
            doc.next_free_id(),
            "Title",
            NodeKind::Text(TextData {
                content: "Hello".into(),
                ..TextData::default()
            }),
            Rect::new(0.0, 0.0, 50.0, 20.0),
        );
        t.effects.push(Effect::DropShadow {
            color: Rgba::BLACK,
            offset: Point::new(0.0, 4.0),
            blur: 8.0,
            spread: 0.0,
            visible: true,
        });
        t.fills.push(Paint::linear(
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            vec![ColorStop {
                position: 0.0,
                color: Rgba::WHITE,
            }],
        ));
        doc.insert(t, page, 1).unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"type\":\"frame\""));
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
        back.validate().unwrap();
    }
}
