use deesainer_doc::{Node, NodeId, NodeKind, PropValue, Segment, VectorNetwork, VectorPath};
use deesainer_geom::{Point, Rect};
use deesainer_ops::{Op, Transaction};
use serde_json::{json, Value};

use super::param;
use super::vector::fit_path;
use crate::error::EngineError;
use crate::params::Params;
use crate::registry::{CommandSpec, Context, Outcome, ParamType};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("vector.getNetwork", "Get vector network", get_network)
            .read_only()
            .doc("Returns the layer's vector network: {vertices: [{x,y}], segments: [{a,b,ta,tb}], regions}. A layer without one is read as a network built from its path.")
            .params(const { &[param("id", ParamType::NodeId, true, "Vector layer.")] }),
        CommandSpec::new("vector.toNetwork", "Edit as network", to_network)
            .doc("Keeps a vector layer as a network (vertices and segments) so that more than two segments can meet at a vertex. Returns {vertices, segments, regions}.")
            .params(const { &[param("id", ParamType::NodeId, true, "Vector layer.")] }),
        CommandSpec::new("vector.setNetwork", "Set vector network", set_network)
            .doc("Replaces a layer's network. segments join vertex indices a and b; ta and tb are the tangent offsets from each end vertex (omit for a straight line). Regions (the filled loops) are found automatically.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("vertices", ParamType::Json, true, "[{x, y}] in the layer's local space."),
                    param("segments", ParamType::Json, true, "[{a, b, ta?, tb?}]."),
                ]
            }),
        CommandSpec::new("vector.setRegionFill", "Fill region", set_region_fill)
            .doc("Gives one region of a vector network its own fills, or with fills null returns it to the layer's fills. region is an index into the regions listed by vector.getNetwork; the paint stays with the region while other parts of the network change, and is dropped when the region itself is split or removed.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("region", ParamType::Number, true, "Region index."),
                    param("fills", ParamType::Json, false, "Paints [{type, color, ...}]; omit or null to reset."),
                ]
            }),
        CommandSpec::new("vector.addVertex", "Add vertex", add_vertex)
            .doc("Adds an unconnected vertex to the layer's network. Returns {vertex}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("x", ParamType::Number, true, "Local x."),
                    param("y", ParamType::Number, true, "Local y."),
                ]
            }),
        CommandSpec::new("vector.connect", "Connect vertices", connect)
            .doc("Joins two vertices with a straight segment; regions are recomputed, so closing a loop fills it. Returns {segment}.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("a", ParamType::Number, true, "First vertex index."),
                    param("b", ParamType::Number, true, "Second vertex index."),
                ]
            }),
        CommandSpec::new("vector.moveVertex", "Move vertex", move_vertex)
            .doc("Moves a vertex; the curves of its segments follow.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("vertex", ParamType::Number, true, "Vertex index."),
                    param("x", ParamType::Number, true, "Local x."),
                    param("y", ParamType::Number, true, "Local y."),
                ]
            }),
        CommandSpec::new("vector.setTangents", "Bend segment", set_tangents)
            .doc("Sets a segment's tangent offsets ({x,y} from the end vertices); both zero makes it straight.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("segment", ParamType::Number, true, "Segment index."),
                    param("ta", ParamType::Json, false, "{x, y} offset from vertex a."),
                    param("tb", ParamType::Json, false, "{x, y} offset from vertex b."),
                ]
            }),
        CommandSpec::new("vector.removeSegment", "Remove segment", remove_segment)
            .doc("Deletes one segment; regions are recomputed.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("segment", ParamType::Number, true, "Segment index."),
                ]
            }),
        CommandSpec::new("vector.removeVertex", "Remove vertex", remove_vertex)
            .doc("Deletes a vertex and the segments that touch it; later indices shift down by one.")
            .params(const {
                &[
                    param("id", ParamType::NodeId, true, "Vector layer."),
                    param("vertex", ParamType::Number, true, "Vertex index."),
                ]
            }),
    ]
}

fn vector_of<'a>(
    ctx: &'a Context<'_>,
    p: &Params<'_>,
) -> Result<(NodeId, &'a Node, &'a VectorPath), EngineError> {
    let id = p.req_node_id("id")?;
    let node = ctx
        .doc
        .get(id)
        .ok_or_else(|| p.bad("id", format!("{id} does not exist")))?;
    match &node.kind {
        NodeKind::Vector { path } => Ok((id, node, path)),
        _ => Err(p.bad("id", "not a vector layer")),
    }
}

fn current(node: &Node, path: &VectorPath) -> VectorNetwork {
    match &node.network {
        Some(net) if net.matches(path) => net.clone(),
        _ => VectorNetwork::from_path(path),
    }
}

fn summary(net: &VectorNetwork) -> Value {
    let regions: Vec<Value> = net
        .regions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut segments: Vec<usize> = r.iter().map(|e| e.segment).collect();
            segments.sort_unstable();
            segments.dedup();
            json!({ "segments": segments, "ownFill": net.paints_of(i).is_some() })
        })
        .collect();
    json!({
        "vertices": net.vertices,
        "segments": net.segments,
        "regions": net.regions.len(),
        "regionList": regions,
    })
}

fn index(p: &Params<'_>, key: &str) -> Result<usize, EngineError> {
    p.u32(key)?
        .map(|v| v as usize)
        .ok_or_else(|| p.missing(key))
}

fn point(p: &Params<'_>, key: &str) -> Result<Option<Point>, EngineError> {
    p.json::<Point>(key)
}

fn commit(
    node: &Node,
    id: NodeId,
    p: &Params<'_>,
    net: VectorNetwork,
    label: &str,
    reply: Value,
) -> Result<Outcome, EngineError> {
    if !net.is_valid() {
        return Err(p.invalid("the network has invalid indices or numbers"));
    }
    let NodeKind::Vector { path: old } = &node.kind else {
        return Err(p.bad("id", "not a vector layer"));
    };
    let path = net.to_path(old.fill_rule);
    let mut ops = Vec::new();
    let net = if path.cmds.is_empty() {
        ops.push(Op::set(
            id,
            PropValue::Kind(NodeKind::Vector { path: path.clone() }),
        ));
        net
    } else {
        let (b, local) = fit_path(&path).ok_or_else(|| p.invalid("the network has no extent"))?;
        ops.push(Op::set(
            id,
            PropValue::Kind(NodeKind::Vector { path: local }),
        ));
        ops.push(Op::set(
            id,
            PropValue::Rect(Rect::new(
                node.rect.x + b.x,
                node.rect.y + b.y,
                b.width,
                b.height,
            )),
        ));
        net.translated(-b.x, -b.y)
    };
    if !net.regions.is_empty() && node.fills.is_empty() {
        ops.push(Op::set(
            id,
            PropValue::Fills(vec![deesainer_doc::Paint::solid(super::create::SHAPE_FILL)]),
        ));
    }
    let in_region: std::collections::BTreeSet<usize> =
        net.regions.iter().flatten().map(|e| e.segment).collect();
    if node.stroke.align == deesainer_doc::StrokeAlign::Inside
        && (0..net.segments.len()).any(|i| !in_region.contains(&i))
    {
        let mut stroke = node.stroke.clone();
        stroke.align = deesainer_doc::StrokeAlign::Center;
        ops.push(Op::set(id, PropValue::Stroke(stroke)));
    }
    ops.push(Op::set(id, PropValue::Network(Some(net))));
    Ok(Outcome::apply(Transaction::new(label, ops), reply))
}

fn get_network(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (_, node, path) = vector_of(ctx, p)?;
    Ok(Outcome::read(summary(&current(node, path))))
}

fn to_network(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let net = current(node, path);
    let reply = summary(&net);
    commit(node, id, p, net, "Edit as network", reply)
}

fn set_network(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, _) = vector_of(ctx, p)?;
    let vertices: Vec<Point> = p.req_json("vertices")?;
    let segments: Vec<Segment> = p.req_json("segments")?;
    let mut net = VectorNetwork {
        vertices,
        segments,
        ..VectorNetwork::default()
    };
    if !net.is_valid() {
        return Err(p.invalid("the network has invalid indices or numbers"));
    }
    net.rebuild_regions();
    let reply = summary(&net);
    commit(node, id, p, net, "Set vector network", reply)
}

fn set_region_fill(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    let region = index(p, "region")?;
    let fills: Option<Vec<deesainer_doc::Paint>> = p.json("fills")?;
    if let Some(list) = &fills {
        if list.len() > 16 {
            return Err(p.bad("fills", "at most 16 paints"));
        }
        super::validate_paints(p, "fills", list)?;
    }
    if !net.set_region_paints(region, fills) {
        return Err(p.bad("region", "no such region"));
    }
    let reply = summary(&net);
    commit(node, id, p, net, "Fill region", reply)
}

fn add_vertex(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    let at = Point::new(p.req_f64("x")?, p.req_f64("y")?);
    let vertex = net.add_vertex(at);
    commit(node, id, p, net, "Add vertex", json!({ "vertex": vertex }))
}

fn connect(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    let (a, b) = (index(p, "a")?, index(p, "b")?);
    let segment = net
        .connect(a, b)
        .ok_or_else(|| p.invalid("needs two different existing vertices"))?;
    commit(
        node,
        id,
        p,
        net,
        "Connect vertices",
        json!({ "segment": segment }),
    )
}

fn move_vertex(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    let at = Point::new(p.req_f64("x")?, p.req_f64("y")?);
    if !net.move_vertex(index(p, "vertex")?, at) {
        return Err(p.bad("vertex", "no such vertex"));
    }
    commit(node, id, p, net, "Move vertex", json!({}))
}

fn set_tangents(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    let i = index(p, "segment")?;
    let (ta, tb) = (point(p, "ta")?, point(p, "tb")?);
    let seg = net
        .segments
        .get_mut(i)
        .ok_or_else(|| p.bad("segment", "no such segment"))?;
    if let Some(t) = ta {
        seg.ta = t;
    }
    if let Some(t) = tb {
        seg.tb = t;
    }
    net.rebuild_regions();
    commit(node, id, p, net, "Bend segment", json!({}))
}

fn remove_segment(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    if !net.remove_segment(index(p, "segment")?) {
        return Err(p.bad("segment", "no such segment"));
    }
    commit(node, id, p, net, "Remove segment", json!({}))
}

fn remove_vertex(ctx: &Context<'_>, p: &Params<'_>) -> Result<Outcome, EngineError> {
    let (id, node, path) = vector_of(ctx, p)?;
    let mut net = current(node, path);
    if !net.remove_vertex(index(p, "vertex")?) {
        return Err(p.bad("vertex", "no such vertex"));
    }
    commit(node, id, p, net, "Remove vertex", json!({}))
}
