use std::collections::BTreeMap;

use deesainer_color::Rgba;
use serde::{Deserialize, Serialize};

use crate::ids::{CollectionId, StyleId, VariableId};
use crate::paint::{Effect, Paint};
use crate::text::TextStyle;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum StyleKind {
    Paint(Vec<Paint>),
    Text(TextStyle),
    Effect(Vec<Effect>),
    Grid(Vec<crate::LayoutGrid>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: StyleKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<LibraryLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLink {
    pub library: String,
    pub source: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleRefs {
    pub fill: Option<StyleId>,
    pub stroke: Option<StyleId>,
    pub effect: Option<StyleId>,
    pub text: Option<StyleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<StyleId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum VarValue {
    Color(Rgba),
    Number(f64),
    String(String),
    Bool(bool),
    Alias(VariableId),
}

impl VarValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            VarValue::Color(_) => "color",
            VarValue::Number(_) => "number",
            VarValue::String(_) => "string",
            VarValue::Bool(_) => "bool",
            VarValue::Alias(_) => "alias",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    pub name: String,
    pub values: Vec<VarValue>,
    #[serde(default, skip_serializing_if = "CollectionId::is_default")]
    pub collection: CollectionId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<BindTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<LibraryLink>,
}

impl Variable {
    pub fn allows(&self, target: BindTarget) -> bool {
        self.scopes.is_empty() || self.scopes.contains(&target)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Modes {
    #[serde(default = "default_collection_name")]
    pub name: String,
    pub names: Vec<String>,
    pub active: usize,
}

fn default_collection_name() -> String {
    "Variables".to_owned()
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            name: default_collection_name(),
            names: vec!["Default".to_owned()],
            active: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindTarget {
    FillColor,
    StrokeColor,
    Opacity,
    CornerRadius,
    Width,
    Height,
    Gap,
    Padding,
    TextContent,
    Visible,
}

pub type Bindings = BTreeMap<BindTarget, VariableId>;
