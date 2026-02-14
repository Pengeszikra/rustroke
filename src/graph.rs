//! Incremental graph connectivity tracker for filtering closed components.
//! A component is "closed" if every node has even degree (odd_degree_count == 0).

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

const SNAP_EPSILON: f32 = 2.0;

/// Snapped grid key for endpoint deduplication
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapKey(i32, i32);

impl SnapKey {
    pub fn from_point(x: f32, y: f32) -> Self {
        let snap_x = (x / SNAP_EPSILON + if x >= 0.0 { 0.5 } else { -0.5 }) as i32;
        let snap_y = (y / SNAP_EPSILON + if y >= 0.0 { 0.5 } else { -0.5 }) as i32;
        SnapKey(snap_x, snap_y)
    }
}

pub type NodeId = usize;
pub type EdgeId = usize;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Node {
    pub key: SnapKey,
    pub x: f32,
    pub y: f32,
    pub degree: usize,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Edge {
    pub a: NodeId,
    pub b: NodeId,
    pub ax: f32,
    pub ay: f32,
    pub bx: f32,
    pub by: f32,
    pub line_idx: usize,
}

/// Disjoint Set Union (Union-Find) with path compression
#[derive(Debug, Clone)]
struct DSU {
    parent: Vec<usize>,
    size: Vec<usize>,
    odd_count: Vec<usize>,
}

impl DSU {
    fn new() -> Self {
        DSU {
            parent: Vec::new(),
            size: Vec::new(),
            odd_count: Vec::new(),
        }
    }

    fn make_set(&mut self) -> usize {
        let id = self.parent.len();
        self.parent.push(id);
        self.size.push(1);
        self.odd_count.push(0);
        id
    }

    fn find(&mut self, mut x: usize) -> usize {
        if x >= self.parent.len() {
            return x;
        }
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        while x != root {
            let next = self.parent[x];
            self.parent[x] = root;
            x = next;
        }
        root
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }

        let (small, large) = if self.size[rx] < self.size[ry] {
            (rx, ry)
        } else {
            (ry, rx)
        };

        self.parent[small] = large;
        self.size[large] += self.size[small];
        self.odd_count[large] += self.odd_count[small];
    }

    #[allow(dead_code)]
    fn get_odd_count(&mut self, x: usize) -> usize {
        let root = self.find(x);
        self.odd_count[root]
    }

    fn increment_odd_count(&mut self, x: usize) {
        let root = self.find(x);
        self.odd_count[root] += 1;
    }

    fn decrement_odd_count(&mut self, x: usize) {
        let root = self.find(x);
        if self.odd_count[root] > 0 {
            self.odd_count[root] -= 1;
        }
    }
}

/// Incremental graph store for tracking closed components
#[derive(Debug, Clone)]
pub struct GraphStore {
    nodes: Vec<Node>,
    node_map: BTreeMap<SnapKey, NodeId>,
    edges: Vec<Edge>,
    dsu: DSU,
}

impl GraphStore {
    pub fn new() -> Self {
        GraphStore {
            nodes: Vec::new(),
            node_map: BTreeMap::new(),
            edges: Vec::new(),
            dsu: DSU::new(),
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.node_map.clear();
        self.edges.clear();
        self.dsu = DSU::new();
    }

    fn get_or_create_node(&mut self, x: f32, y: f32) -> NodeId {
        let key = SnapKey::from_point(x, y);
        
        if let Some(&id) = self.node_map.get(&key) {
            return id;
        }

        let id = self.nodes.len();
        self.nodes.push(Node {
            key,
            x,
            y,
            degree: 0,
        });
        self.node_map.insert(key, id);
        self.dsu.make_set();
        id
    }

    pub fn add_segment(&mut self, ax: f32, ay: f32, bx: f32, by: f32, line_idx: usize) -> EdgeId {
        let node_a = self.get_or_create_node(ax, ay);
        let node_b = self.get_or_create_node(bx, by);

        for &node_id in &[node_a, node_b] {
            let old_degree = self.nodes[node_id].degree;
            let old_parity = old_degree % 2;
            
            self.nodes[node_id].degree += 1;
            
            let new_parity = self.nodes[node_id].degree % 2;
            
            if old_parity == 0 && new_parity == 1 {
                self.dsu.increment_odd_count(node_id);
            } else if old_parity == 1 && new_parity == 0 {
                self.dsu.decrement_odd_count(node_id);
            }
        }

        if node_a != node_b {
            self.dsu.union(node_a, node_b);
        }

        let edge_id = self.edges.len();
        self.edges.push(Edge {
            a: node_a,
            b: node_b,
            ax,
            ay,
            bx,
            by,
            line_idx,
        });

        edge_id
    }

    #[allow(dead_code)]
    pub fn is_edge_closed(&mut self, edge_id: EdgeId) -> bool {
        if edge_id >= self.edges.len() {
            return false;
        }
        let edge = &self.edges[edge_id];
        let odd_count = self.dsu.get_odd_count(edge.a);
        odd_count == 0
    }

    #[allow(dead_code)]
    pub fn closed_edges(&mut self) -> Vec<EdgeId> {
        let mut result = Vec::new();
        for i in 0..self.edges.len() {
            if self.is_edge_closed(i) {
                result.push(i);
            }
        }
        result
    }

    #[allow(dead_code)]
    pub fn get_edge(&self, edge_id: EdgeId) -> Option<&Edge> {
        self.edges.get(edge_id)
    }

    #[allow(dead_code)]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    #[allow(dead_code)]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterator over nodes (for debug checks)
    #[allow(dead_code)]
    pub fn nodes_iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter()
    }

    /// Iterator over edges (for debug checks)
    #[allow(dead_code)]
    pub fn edges_iter(&self) -> impl Iterator<Item = (usize, &Edge)> + '_ {
        self.edges.iter().enumerate()
    }

    /// Debug-only: Check DSU parent pointer integrity
    #[allow(dead_code)]
    pub fn check_dsu_integrity(&mut self) {
        #[cfg(debug_assertions)]
        {
            for node_id in 0..self.nodes.len() {
                let root = self.dsu.find(node_id);
                // Root's parent must point to itself
                debug_assert!(
                    root < self.dsu.parent.len(),
                    "DSU find() returned invalid root: {} >= {}",
                    root,
                    self.dsu.parent.len()
                );
                debug_assert_eq!(
                    self.dsu.parent[root],
                    root,
                    "DSU root {} has parent {} (should be self)",
                    root,
                    self.dsu.parent[root]
                );
                // Size must be > 0 for root
                debug_assert!(
                    self.dsu.size[root] > 0,
                    "DSU root {} has zero size",
                    root
                );
            }
        }
    }

    /// Debug-only: Check component odd_count matches actual odd-degree nodes
    #[allow(dead_code)]
    pub fn check_component_parity(&mut self) {
        #[cfg(debug_assertions)]
        {
            use alloc::collections::BTreeMap;
            
            // Count actual odd-degree nodes per component
            let mut actual_odd: BTreeMap<usize, usize> = BTreeMap::new();
            for node_id in 0..self.nodes.len() {
                let root = self.dsu.find(node_id);
                let degree = self.nodes[node_id].degree;
                if degree % 2 == 1 {
                    *actual_odd.entry(root).or_insert(0) += 1;
                }
            }

            // Compare with DSU tracked odd_count
            for (root, count) in actual_odd.iter() {
                let tracked = self.dsu.odd_count[*root];
                debug_assert_eq!(
                    tracked,
                    *count,
                    "Component {} has odd_count mismatch: tracked={} actual={}",
                    root,
                    tracked,
                    count
                );
            }

            // Check that components with zero odd nodes are marked as such
            for root in 0..self.nodes.len() {
                if self.dsu.parent[root] == root {
                    let actual = actual_odd.get(&root).copied().unwrap_or(0);
                    let tracked = self.dsu.odd_count[root];
                    debug_assert_eq!(
                        tracked,
                        actual,
                        "Root component {} odd_count mismatch: tracked={} actual={}",
                        root,
                        tracked,
                        actual
                    );
                }
            }
        }
    }
}
