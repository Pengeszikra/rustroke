//! Debug-only integrity checks for detecting corrupted state early.
//! All functions are no-ops in release builds.

use crate::graph::GraphStore;
use crate::{Editor, FillGraph};

/// Check graph_store DSU and component invariants
#[allow(unused_variables, dead_code)]
pub fn check_graph_integrity(graph: &mut GraphStore) {
    #[cfg(debug_assertions)]
    {
        // Check 1: All coordinates are finite
        for node in graph.nodes_iter() {
            debug_assert!(
                node.x.is_finite() && node.y.is_finite(),
                "Graph node has non-finite coordinates: ({}, {})",
                node.x,
                node.y
            );
        }

        // Check 2: All edge endpoints are valid node IDs
        for (edge_id, edge) in graph.edges_iter() {
            debug_assert!(
                edge.a < graph.node_count(),
                "Edge {} has invalid node_a: {} >= {}",
                edge_id,
                edge.a,
                graph.node_count()
            );
            debug_assert!(
                edge.b < graph.node_count(),
                "Edge {} has invalid node_b: {} >= {}",
                edge_id,
                edge.b,
                graph.node_count()
            );
        }

        // Check 3: DSU parent pointers are valid
        graph.check_dsu_integrity();

        // Check 4: Component odd_count matches actual odd-degree nodes
        graph.check_component_parity();
    }
}

/// Check fill_graph structure (nodes, segments, half-edges)
#[allow(unused_variables)]
pub fn check_fill_graph_integrity(fill_graph: &FillGraph) {
    #[cfg(debug_assertions)]
    {
        // Check 1: All node coordinates are finite
        for (idx, node) in fill_graph.nodes.iter().enumerate() {
            debug_assert!(
                node.x.is_finite() && node.y.is_finite(),
                "FillGraph node {} has non-finite coordinates: ({}, {})",
                idx,
                node.x,
                node.y
            );
        }

        // Check 2: All segments reference valid nodes
        for (seg_idx, seg) in fill_graph.segments.iter().enumerate() {
            debug_assert!(
                (seg.a as usize) < fill_graph.nodes.len(),
                "Segment {} has invalid node_a: {} >= {}",
                seg_idx,
                seg.a,
                fill_graph.nodes.len()
            );
            debug_assert!(
                (seg.b as usize) < fill_graph.nodes.len(),
                "Segment {} has invalid node_b: {} >= {}",
                seg_idx,
                seg.b,
                fill_graph.nodes.len()
            );
            debug_assert!(
                seg.a != seg.b,
                "Segment {} is degenerate (a == b == {})",
                seg_idx,
                seg.a
            );
        }

        // Check 3: Half-edges reference valid nodes and segments
        for (he_idx, he) in fill_graph.half_edges.iter().enumerate() {
            debug_assert!(
                (he.from as usize) < fill_graph.nodes.len(),
                "HalfEdge {} has invalid from: {} >= {}",
                he_idx,
                he.from,
                fill_graph.nodes.len()
            );
            debug_assert!(
                (he.to as usize) < fill_graph.nodes.len(),
                "HalfEdge {} has invalid to: {} >= {}",
                he_idx,
                he.to,
                fill_graph.nodes.len()
            );
            debug_assert!(
                (he.seg as usize) < fill_graph.segments.len(),
                "HalfEdge {} has invalid seg: {} >= {}",
                he_idx,
                he.seg,
                fill_graph.segments.len()
            );
        }

        // Check 4: Outgoing lists reference valid half-edges
        for (node_idx, outgoing) in fill_graph.outgoing.iter().enumerate() {
            for &he_idx in outgoing {
                debug_assert!(
                    (he_idx as usize) < fill_graph.half_edges.len(),
                    "Node {} outgoing list has invalid HE index: {} >= {}",
                    node_idx,
                    he_idx,
                    fill_graph.half_edges.len()
                );
            }
        }
    }
}

/// Check line coordinates are valid
#[allow(unused_variables)]
pub fn check_line_coordinates(x1: f32, y1: f32, x2: f32, y2: f32) {
    #[cfg(debug_assertions)]
    {
        debug_assert!(x1.is_finite(), "Line x1 is not finite: {}", x1);
        debug_assert!(y1.is_finite(), "Line y1 is not finite: {}", y1);
        debug_assert!(x2.is_finite(), "Line x2 is not finite: {}", x2);
        debug_assert!(y2.is_finite(), "Line y2 is not finite: {}", y2);
    }
}

/// Check editor state consistency
#[allow(unused_variables)]
pub fn check_editor_integrity(editor: &mut Editor) {
    #[cfg(debug_assertions)]
    {
        // Check 1: All lines have finite coordinates
        for (idx, line) in editor.lines.iter().enumerate() {
            debug_assert!(
                line.x1.is_finite() && line.y1.is_finite() && 
                line.x2.is_finite() && line.y2.is_finite(),
                "Line {} has non-finite coordinates: ({},{}) -> ({},{})",
                idx, line.x1, line.y1, line.x2, line.y2
            );
        }

        // Check 2: Fill polygons have finite coordinates
        for (idx, poly) in editor.fills.iter().enumerate() {
            for (pt_idx, &x) in poly.points.iter().step_by(2).enumerate() {
                let y = poly.points.get(pt_idx * 2 + 1).copied().unwrap_or(0.0);
                debug_assert!(
                    x.is_finite() && y.is_finite(),
                    "Fill {} has non-finite point {}: ({}, {})",
                    idx,
                    pt_idx,
                    x,
                    y
                );
            }
        }

        // Check 3: Graph store integrity
        check_graph_integrity(&mut editor.graph_store);

        // Check 4: Fill graph integrity
        check_fill_graph_integrity(&editor.fill_graph);
    }
}
