use std::collections::BTreeMap;

use deesainer_color::BlendMode;
use deesainer_geom::Rect;
use serde::{Deserialize, Serialize};

use crate::ids::NodeId;
use crate::layout::{AutoLayout, Constraints, LayoutChild};
use crate::paint::{Effect, Paint, StrokeStyle};
use crate::proto::Interaction;
use crate::style::{Bindings, StyleRefs};
use crate::text::TextData;
use crate::vector::VectorPath;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Override {
    pub target: NodeId,
    pub value: PropValue,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeKind {
    Document,
    Page,
    Frame,
    Group,
    Component {
        #[serde(default)]
        description: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        properties: Vec<ComponentProp>,
    },
    ComponentSet,
    Instance {
        component: NodeId,
        #[serde(default)]
        overrides: Vec<Override>,
    },
    SlotContent {
        slot: String,
    },
    Rectangle,
    Ellipse {
        #[serde(default)]
        arc_start: f64,
        #[serde(default = "full_arc")]
        arc_end: f64,
        #[serde(default)]
        inner_ratio: f64,
    },
    Line,
    Polygon {
        sides: u32,
    },
    Star {
        points: u32,
        inner_ratio: f64,
    },
    Vector {
        path: VectorPath,
    },
    Text(TextData),
    BooleanOp {
        op: BoolOp,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ComponentPropKind {
    Boolean {
        default: bool,
    },
    Text {
        default: String,
    },
    InstanceSwap {
        default: NodeId,
    },
    Slot {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        preferred: Vec<NodeId>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentProp {
    pub name: String,
    pub target: NodeId,
    #[serde(flatten)]
    pub kind: ComponentPropKind,
}

impl ComponentProp {
    pub const MAX: usize = 32;
}

fn full_arc() -> f64 {
    360.0
}

impl NodeKind {
    pub fn ellipse() -> Self {
        NodeKind::Ellipse {
            arc_start: 0.0,
            arc_end: 360.0,
            inner_ratio: 0.0,
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(
            self,
            NodeKind::Document
                | NodeKind::Page
                | NodeKind::Frame
                | NodeKind::Group
                | NodeKind::Component { .. }
                | NodeKind::ComponentSet
                | NodeKind::BooleanOp { .. }
                | NodeKind::SlotContent { .. }
        )
    }

    pub fn is_frame_like(&self) -> bool {
        matches!(
            self,
            NodeKind::Frame
                | NodeKind::Component { .. }
                | NodeKind::ComponentSet
                | NodeKind::Instance { .. }
                | NodeKind::SlotContent { .. }
        )
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Document => "document",
            NodeKind::Page => "page",
            NodeKind::Frame => "frame",
            NodeKind::Group => "group",
            NodeKind::Component { .. } => "component",
            NodeKind::ComponentSet => "componentSet",
            NodeKind::Instance { .. } => "instance",
            NodeKind::SlotContent { .. } => "slotContent",
            NodeKind::Rectangle => "rectangle",
            NodeKind::Ellipse { .. } => "ellipse",
            NodeKind::Line => "line",
            NodeKind::Polygon { .. } => "polygon",
            NodeKind::Star { .. } => "star",
            NodeKind::Vector { .. } => "vector",
            NodeKind::Text(_) => "text",
            NodeKind::BooleanOp { .. } => "booleanOp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerRadius {
    pub top_left: f64,
    pub top_right: f64,
    pub bottom_right: f64,
    pub bottom_left: f64,
}

impl CornerRadius {
    pub const fn uniform(r: f64) -> Self {
        Self {
            top_left: r,
            top_right: r,
            bottom_right: r,
            bottom_left: r,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.top_left == 0.0
            && self.top_right == 0.0
            && self.bottom_right == 0.0
            && self.bottom_left == 0.0
    }

    pub fn is_valid(&self) -> bool {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .iter()
        .all(|r| r.is_finite() && *r >= 0.0)
    }
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    #[serde(default)]
    pub children: Vec<NodeId>,
    pub rect: Rect,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default)]
    pub fills: Vec<Paint>,
    #[serde(default)]
    pub strokes: Vec<Paint>,
    #[serde(default)]
    pub stroke: StrokeStyle,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub corner_radius: CornerRadius,
    #[serde(default)]
    pub constraints: Constraints,
    #[serde(default)]
    pub layout: Option<AutoLayout>,
    #[serde(default)]
    pub layout_child: LayoutChild,
    #[serde(default)]
    pub clips_content: bool,
    #[serde(default)]
    pub styles: StyleRefs,
    #[serde(default)]
    pub bindings: Bindings,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
    #[serde(default)]
    pub is_mask: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_grids: Vec<crate::LayoutGrid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<crate::RulerGuide>,
    #[serde(default, skip_serializing_if = "crate::Device::is_none")]
    pub device: crate::Device,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    #[serde(default, skip_serializing_if = "crate::Scrolling::is_none")]
    pub scrolling: crate::Scrolling,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variable_modes: BTreeMap<crate::CollectionId, usize>,
    #[serde(default, skip_serializing_if = "crate::SizeLimits::is_none")]
    pub size_limits: crate::SizeLimits,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub export_settings: Vec<crate::ExportSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<LibraryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<crate::VectorNetwork>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRef {
    pub library: String,
    pub source: NodeId,
}

impl Node {
    pub fn new(id: NodeId, name: impl Into<String>, kind: NodeKind, rect: Rect) -> Self {
        let clips_content = matches!(kind, NodeKind::Frame | NodeKind::Component { .. });
        Self {
            id,
            name: name.into(),
            kind,
            parent: None,
            children: Vec::new(),
            rect,
            rotation: 0.0,
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: BlendMode::PassThrough,
            fills: Vec::new(),
            strokes: Vec::new(),
            stroke: StrokeStyle::default(),
            effects: Vec::new(),
            corner_radius: CornerRadius::default(),
            constraints: Constraints::default(),
            layout: None,
            layout_child: LayoutChild::default(),
            clips_content,
            styles: StyleRefs::default(),
            bindings: Bindings::default(),
            interactions: Vec::new(),
            is_mask: false,
            layout_grids: Vec::new(),
            guides: Vec::new(),
            export_settings: Vec::new(),
            size_limits: crate::SizeLimits::default(),
            variable_modes: BTreeMap::new(),
            scrolling: crate::Scrolling::default(),
            annotations: Vec::new(),
            device: crate::Device::None,
            origin: None,
            network: None,
        }
    }

    pub fn fill_parts<'a>(
        &'a self,
        outline: &'a VectorPath,
    ) -> Vec<(std::borrow::Cow<'a, VectorPath>, &'a [Paint])> {
        use std::borrow::Cow;
        if let (NodeKind::Vector { .. }, Some(net)) = (&self.kind, &self.network) {
            if !net.region_paints.is_empty() && net.matches(outline) {
                return (0..net.regions.len())
                    .filter_map(|i| {
                        let path = net.region_path(i, outline.fill_rule)?;
                        Some((
                            Cow::Owned(path),
                            net.paints_of(i).unwrap_or(self.fills.as_slice()),
                        ))
                    })
                    .collect();
            }
        }
        vec![(Cow::Borrowed(outline), self.fills.as_slice())]
    }

    pub fn text(&self) -> Option<&TextData> {
        match &self.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn get(&self, key: PropKey) -> PropValue {
        match key {
            PropKey::Name => PropValue::Name(self.name.clone()),
            PropKey::Kind => PropValue::Kind(self.kind.clone()),
            PropKey::Rect => PropValue::Rect(self.rect),
            PropKey::Rotation => PropValue::Rotation(self.rotation),
            PropKey::Visible => PropValue::Visible(self.visible),
            PropKey::Locked => PropValue::Locked(self.locked),
            PropKey::Opacity => PropValue::Opacity(self.opacity),
            PropKey::Blend => PropValue::Blend(self.blend),
            PropKey::Fills => PropValue::Fills(self.fills.clone()),
            PropKey::Strokes => PropValue::Strokes(self.strokes.clone()),
            PropKey::Stroke => PropValue::Stroke(self.stroke.clone()),
            PropKey::Effects => PropValue::Effects(self.effects.clone()),
            PropKey::CornerRadius => PropValue::CornerRadius(self.corner_radius),
            PropKey::Constraints => PropValue::Constraints(self.constraints),
            PropKey::Layout => PropValue::Layout(self.layout),
            PropKey::LayoutChild => PropValue::LayoutChild(self.layout_child),
            PropKey::ClipsContent => PropValue::ClipsContent(self.clips_content),
            PropKey::Styles => PropValue::Styles(self.styles),
            PropKey::Bindings => PropValue::Bindings(self.bindings.clone()),
            PropKey::Interactions => PropValue::Interactions(self.interactions.clone()),
            PropKey::IsMask => PropValue::IsMask(self.is_mask),
            PropKey::LayoutGrids => PropValue::LayoutGrids(self.layout_grids.clone()),
            PropKey::Guides => PropValue::Guides(self.guides.clone()),
            PropKey::ExportSettings => PropValue::ExportSettings(self.export_settings.clone()),
            PropKey::SizeLimits => PropValue::SizeLimits(self.size_limits),
            PropKey::VariableModes => PropValue::VariableModes(self.variable_modes.clone()),
            PropKey::Scrolling => PropValue::Scrolling(self.scrolling),
            PropKey::Annotations => PropValue::Annotations(self.annotations.clone()),
            PropKey::Device => PropValue::Device(self.device),
            PropKey::Origin => PropValue::Origin(self.origin.clone()),
            PropKey::Network => PropValue::Network(self.network.clone()),
        }
    }

    pub fn set(&mut self, value: PropValue) -> PropValue {
        fn swap<T>(slot: &mut T, v: T) -> T {
            std::mem::replace(slot, v)
        }
        match value {
            PropValue::Name(v) => PropValue::Name(swap(&mut self.name, v)),
            PropValue::Kind(v) => PropValue::Kind(swap(&mut self.kind, v)),
            PropValue::Rect(v) => PropValue::Rect(swap(&mut self.rect, v)),
            PropValue::Rotation(v) => PropValue::Rotation(swap(&mut self.rotation, v)),
            PropValue::Visible(v) => PropValue::Visible(swap(&mut self.visible, v)),
            PropValue::Locked(v) => PropValue::Locked(swap(&mut self.locked, v)),
            PropValue::Opacity(v) => PropValue::Opacity(swap(&mut self.opacity, v)),
            PropValue::Blend(v) => PropValue::Blend(swap(&mut self.blend, v)),
            PropValue::Fills(v) => PropValue::Fills(swap(&mut self.fills, v)),
            PropValue::Strokes(v) => PropValue::Strokes(swap(&mut self.strokes, v)),
            PropValue::Stroke(v) => PropValue::Stroke(swap(&mut self.stroke, v)),
            PropValue::Effects(v) => PropValue::Effects(swap(&mut self.effects, v)),
            PropValue::CornerRadius(v) => PropValue::CornerRadius(swap(&mut self.corner_radius, v)),
            PropValue::Constraints(v) => PropValue::Constraints(swap(&mut self.constraints, v)),
            PropValue::Layout(v) => PropValue::Layout(swap(&mut self.layout, v)),
            PropValue::LayoutChild(v) => PropValue::LayoutChild(swap(&mut self.layout_child, v)),
            PropValue::ClipsContent(v) => PropValue::ClipsContent(swap(&mut self.clips_content, v)),
            PropValue::Styles(v) => PropValue::Styles(swap(&mut self.styles, v)),
            PropValue::Bindings(v) => PropValue::Bindings(swap(&mut self.bindings, v)),
            PropValue::Interactions(v) => PropValue::Interactions(swap(&mut self.interactions, v)),
            PropValue::IsMask(v) => PropValue::IsMask(swap(&mut self.is_mask, v)),
            PropValue::LayoutGrids(v) => PropValue::LayoutGrids(swap(&mut self.layout_grids, v)),
            PropValue::Guides(v) => PropValue::Guides(swap(&mut self.guides, v)),
            PropValue::SizeLimits(v) => PropValue::SizeLimits(swap(&mut self.size_limits, v)),
            PropValue::Scrolling(v) => PropValue::Scrolling(swap(&mut self.scrolling, v)),
            PropValue::Annotations(v) => PropValue::Annotations(swap(&mut self.annotations, v)),
            PropValue::Device(v) => PropValue::Device(swap(&mut self.device, v)),
            PropValue::VariableModes(v) => {
                PropValue::VariableModes(swap(&mut self.variable_modes, v))
            }
            PropValue::ExportSettings(v) => {
                PropValue::ExportSettings(swap(&mut self.export_settings, v))
            }
            PropValue::Origin(v) => PropValue::Origin(swap(&mut self.origin, v)),
            PropValue::Network(v) => PropValue::Network(swap(&mut self.network, v)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropKey {
    Name,
    Kind,
    Rect,
    Rotation,
    Visible,
    Locked,
    Opacity,
    Blend,
    Fills,
    Strokes,
    Stroke,
    Effects,
    CornerRadius,
    Constraints,
    Layout,
    LayoutChild,
    ClipsContent,
    Styles,
    Bindings,
    Interactions,
    IsMask,
    LayoutGrids,
    Guides,
    ExportSettings,
    SizeLimits,
    VariableModes,
    Scrolling,
    Annotations,
    Device,
    Origin,
    Network,
}

impl PropKey {
    pub const ALL: [PropKey; 31] = [
        PropKey::Name,
        PropKey::Kind,
        PropKey::Rect,
        PropKey::Rotation,
        PropKey::Visible,
        PropKey::Locked,
        PropKey::Opacity,
        PropKey::Blend,
        PropKey::Fills,
        PropKey::Strokes,
        PropKey::Stroke,
        PropKey::Effects,
        PropKey::CornerRadius,
        PropKey::Constraints,
        PropKey::Layout,
        PropKey::LayoutChild,
        PropKey::ClipsContent,
        PropKey::Styles,
        PropKey::Bindings,
        PropKey::Interactions,
        PropKey::IsMask,
        PropKey::LayoutGrids,
        PropKey::Guides,
        PropKey::ExportSettings,
        PropKey::SizeLimits,
        PropKey::VariableModes,
        PropKey::Scrolling,
        PropKey::Annotations,
        PropKey::Device,
        PropKey::Origin,
        PropKey::Network,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "prop", content = "value", rename_all = "camelCase")]
pub enum PropValue {
    Name(String),
    Kind(NodeKind),
    Rect(Rect),
    Rotation(f64),
    Visible(bool),
    Locked(bool),
    Opacity(f32),
    Blend(BlendMode),
    Fills(Vec<Paint>),
    Strokes(Vec<Paint>),
    Stroke(StrokeStyle),
    Effects(Vec<Effect>),
    CornerRadius(CornerRadius),
    Constraints(Constraints),
    Layout(Option<AutoLayout>),
    LayoutChild(LayoutChild),
    ClipsContent(bool),
    Styles(StyleRefs),
    Bindings(Bindings),
    Interactions(Vec<Interaction>),
    IsMask(bool),
    LayoutGrids(Vec<crate::LayoutGrid>),
    Guides(Vec<crate::RulerGuide>),
    ExportSettings(Vec<crate::ExportSetting>),
    SizeLimits(crate::SizeLimits),
    VariableModes(BTreeMap<crate::CollectionId, usize>),
    Scrolling(crate::Scrolling),
    Annotations(Vec<Annotation>),
    Device(crate::Device),
    Origin(Option<LibraryRef>),
    Network(Option<crate::VectorNetwork>),
}

impl PropValue {
    pub fn key(&self) -> PropKey {
        match self {
            PropValue::Name(_) => PropKey::Name,
            PropValue::Kind(_) => PropKey::Kind,
            PropValue::Rect(_) => PropKey::Rect,
            PropValue::Rotation(_) => PropKey::Rotation,
            PropValue::Visible(_) => PropKey::Visible,
            PropValue::Locked(_) => PropKey::Locked,
            PropValue::Opacity(_) => PropKey::Opacity,
            PropValue::Blend(_) => PropKey::Blend,
            PropValue::Fills(_) => PropKey::Fills,
            PropValue::Strokes(_) => PropKey::Strokes,
            PropValue::Stroke(_) => PropKey::Stroke,
            PropValue::Effects(_) => PropKey::Effects,
            PropValue::CornerRadius(_) => PropKey::CornerRadius,
            PropValue::Constraints(_) => PropKey::Constraints,
            PropValue::Layout(_) => PropKey::Layout,
            PropValue::LayoutChild(_) => PropKey::LayoutChild,
            PropValue::ClipsContent(_) => PropKey::ClipsContent,
            PropValue::Styles(_) => PropKey::Styles,
            PropValue::Bindings(_) => PropKey::Bindings,
            PropValue::Interactions(_) => PropKey::Interactions,
            PropValue::IsMask(_) => PropKey::IsMask,
            PropValue::LayoutGrids(_) => PropKey::LayoutGrids,
            PropValue::Guides(_) => PropKey::Guides,
            PropValue::ExportSettings(_) => PropKey::ExportSettings,
            PropValue::SizeLimits(_) => PropKey::SizeLimits,
            PropValue::VariableModes(_) => PropKey::VariableModes,
            PropValue::Scrolling(_) => PropKey::Scrolling,
            PropValue::Annotations(_) => PropKey::Annotations,
            PropValue::Device(_) => PropKey::Device,
            PropValue::Origin(_) => PropKey::Origin,
            PropValue::Network(_) => PropKey::Network,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<String>,
}
