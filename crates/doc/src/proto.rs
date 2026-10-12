use serde::{Deserialize, Serialize};

use crate::ids::NodeId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Trigger {
    OnClick,
    OnHover,
    OnPress,
    AfterDelay { ms: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    Navigate {
        to: NodeId,
    },
    OpenOverlay {
        frame: NodeId,
    },
    CloseOverlay,
    Back,
    OpenUrl {
        url: String,
    },
    SetVariable {
        variable: crate::VariableId,
        value: crate::VarValue,
    },
    Conditional {
        condition: Condition,
        then: Vec<Action>,
        #[serde(default)]
        otherwise: Vec<Action>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    pub variable: crate::VariableId,
    pub op: CompareOp,
    pub value: crate::VarValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompareOp {
    Equals,
    NotEquals,
    Greater,
    Less,
}

impl Condition {
    pub fn holds(&self, current: &crate::VarValue) -> bool {
        use crate::VarValue as V;
        match (self.op, current, &self.value) {
            (CompareOp::Equals, a, b) => a == b,
            (CompareOp::NotEquals, a, b) => a != b,
            (CompareOp::Greater, V::Number(a), V::Number(b)) => a > b,
            (CompareOp::Less, V::Number(a), V::Number(b)) => a < b,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Easing {
    Linear,
    #[default]
    EaseOut,
    EaseIn,
    EaseInOut,
}

impl Easing {
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TransitionKind {
    #[default]
    Instant,
    Dissolve,
    SmartAnimate,
    MoveIn {
        direction: Direction,
    },
    Push {
        direction: Direction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration_ms: u32,
    pub easing: Easing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    pub trigger: Trigger,
    pub action: Action,
    #[serde(default)]
    pub transition: Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Overflow {
    #[default]
    None,
    Vertical,
    Horizontal,
    Both,
}

impl Overflow {
    pub fn axes(self) -> (bool, bool) {
        match self {
            Overflow::None => (false, false),
            Overflow::Vertical => (false, true),
            Overflow::Horizontal => (true, false),
            Overflow::Both => (true, true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scrolling {
    pub overflow: Overflow,
    pub fixed: bool,
}

impl Scrolling {
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Device {
    #[default]
    None,
    Phone,
    Tablet,
    Browser,
}

impl Device {
    pub fn is_none(&self) -> bool {
        *self == Device::None
    }
}
