use deesainer_geom::Point;
use serde::{Deserialize, Serialize};

use crate::ids::NodeId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub author: String,
    pub text: String,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub created: String,
    pub page: NodeId,
    pub position: Point,
    #[serde(default)]
    pub node: Option<NodeId>,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub replies: Vec<Reply>,
}
