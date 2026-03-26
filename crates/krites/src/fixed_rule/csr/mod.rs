//! Compressed sparse row graph representation.

mod page_rank;

pub(crate) use page_rank::{PageRankConfig, page_rank};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct Target<EV> {
    pub target: u32,
    pub value: EV,
}

impl<EV> Target<EV> {
    pub fn new(target: u32, value: EV) -> Self {
        Self { target, value }
    }
}

impl<EV> Ord for Target<EV> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.target.cmp(&other.target)
    }
}

impl<EV> PartialOrd for Target<EV> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<EV> PartialEq for Target<EV> {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
    }
}

impl<EV> Eq for Target<EV> {}

struct Csr<EV> {
    offsets: Box<[u32]>,
    targets: Box<[Target<EV>]>,
}

impl<EV> Csr<EV> {
    fn node_count(&self) -> u32 {
        #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
        let count = (self.offsets.len() - 1) as u32;
        count
    }

    fn degree(&self, node: u32) -> u32 {
        let i = node as usize;
        self.offsets[i + 1] - self.offsets[i]
    }

    fn targets_with_values(&self, node: u32) -> &[Target<EV>] {
        let i = node as usize;
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let from = self.offsets[i] as usize;
        let to = self.offsets[i + 1] as usize;
        &self.targets[from..to]
    }
}

impl Csr<()> {
    fn targets_iter(&self, node: u32) -> impl Iterator<Item = u32> + '_ {
        let i = node as usize;
        #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
        let from = self.offsets[i] as usize;
        let to = self.offsets[i + 1] as usize;
        self.targets[from..to].iter().map(|t| t.target)
    }
}

pub(crate) struct DirectedCsrGraph<EV = ()> {
    csr_out: Csr<EV>,
    csr_inc: Csr<EV>,
}

impl<EV> DirectedCsrGraph<EV> {
    pub fn node_count(&self) -> u32 {
        self.csr_out.node_count()
    }

    pub fn out_degree(&self, node: u32) -> u32 {
        self.csr_out.degree(node)
    }

    pub fn out_neighbors_with_values(&self, node: u32) -> std::slice::Iter<'_, Target<EV>> {
        self.csr_out.targets_with_values(node).iter()
    }
}

impl DirectedCsrGraph<()> {
    pub fn out_neighbors(&self, node: u32) -> impl Iterator<Item = u32> + '_ {
        self.csr_out.targets_iter(node)
    }

    pub fn in_neighbors(&self, node: u32) -> impl Iterator<Item = u32> + '_ {
        self.csr_inc.targets_iter(node)
    }
}

pub(crate) struct CsrBuilder<EV> {
    edges: Vec<(u32, u32, EV)>,
    sorted: bool,
}

impl<EV> Default for CsrBuilder<EV> {
    fn default() -> Self {
        Self {
            edges: Vec::new(),
            sorted: false,
        }
    }
}

impl<EV: Copy> CsrBuilder<EV> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sorted(mut self) -> Self {
        self.sorted = true;
        self
    }

    pub fn edges_with_values(mut self, edges: impl IntoIterator<Item = (u32, u32, EV)>) -> Self {
        self.edges = edges.into_iter().collect();
        self
    }

    pub fn build(self) -> DirectedCsrGraph<EV> {
        let node_count = self
            .edges
            .iter()
            .map(|(s, t, _)| (*s).max(*t))
            .max()
            .map_or(0, |m| m + 1);

        let csr_out = self.build_csr(node_count, false);
        let csr_inc = self.build_csr(node_count, true);

        DirectedCsrGraph { csr_out, csr_inc }
    }

    fn build_csr(&self, node_count: u32, incoming: bool) -> Csr<EV> {
        let n = node_count as usize;

        let mut adj: Vec<Vec<Target<EV>>> = vec![Vec::new(); n];
        for &(s, t, v) in &self.edges {
            let (key, neighbor) = if incoming { (t, s) } else { (s, t) };
            adj[key as usize].push(Target::new(neighbor, v));
        }

        if self.sorted {
            for list in &mut adj {
                list.sort_unstable();
            }
        }

        let mut offsets = Vec::with_capacity(n + 1);
        let mut targets = Vec::new();
        for list in adj {
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let offset = targets.len() as u32;
            offsets.push(offset);
            targets.extend(list);
        }
        #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
        let final_offset = targets.len() as u32;
        offsets.push(final_offset);

        Csr {
            offsets: offsets.into_boxed_slice(),
            targets: targets.into_boxed_slice(),
        }
    }
}

impl CsrBuilder<()> {
    pub fn edges(mut self, edges: impl IntoIterator<Item = (u32, u32)>) -> Self {
        self.edges = edges.into_iter().map(|(s, t)| (s, t, ())).collect();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle_graph() -> DirectedCsrGraph {
        CsrBuilder::new()
            .sorted()
            .edges([(0, 1), (1, 2), (2, 0)])
            .build()
    }

    #[test]
    fn construction_node_count() {
        let g = triangle_graph();
        assert_eq!(g.node_count(), 3);
    }

    #[test]
    fn out_degree() {
        let g = triangle_graph();
        assert_eq!(g.out_degree(0), 1);
        assert_eq!(g.out_degree(1), 1);
        assert_eq!(g.out_degree(2), 1);
    }

    #[test]
    fn out_neighbors_basic() {
        let g = triangle_graph();
        let out0: Vec<u32> = g.out_neighbors(0).collect();
        assert_eq!(out0, vec![1]);
        let out2: Vec<u32> = g.out_neighbors(2).collect();
        assert_eq!(out2, vec![0]);
    }

    #[test]
    fn in_neighbors_basic() {
        let g = triangle_graph();
        let in0: Vec<u32> = g.in_neighbors(0).collect();
        assert_eq!(in0, vec![2]);
        let in1: Vec<u32> = g.in_neighbors(1).collect();
        assert_eq!(in1, vec![0]);
    }

    #[test]
    fn multi_edge_graph() {
        let g = CsrBuilder::new()
            .sorted()
            .edges([(0, 1), (0, 2), (1, 2)])
            .build();
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.out_degree(0), 2);
        assert_eq!(g.out_degree(1), 1);
        assert_eq!(g.out_degree(2), 0);
        let out0: Vec<u32> = g.out_neighbors(0).collect();
        assert_eq!(out0, vec![1, 2]);
        let in2: Vec<u32> = g.in_neighbors(2).collect();
        assert_eq!(in2, vec![0, 1]);
    }

    #[test]
    fn empty_graph() {
        let g: DirectedCsrGraph = CsrBuilder::new().edges([]).build();
        assert_eq!(g.node_count(), 0);
    }

    #[test]
    fn edge_weight_access() {
        let g: DirectedCsrGraph<f32> = CsrBuilder::new()
            .sorted()
            .edges_with_values([(0, 1, 1.0_f32), (1, 2, 2.0_f32)])
            .build();
        let targets: Vec<(u32, f32)> = g
            .out_neighbors_with_values(0)
            .map(|t| (t.target, t.value))
            .collect();
        assert_eq!(targets, vec![(1, 1.0)]);
        let targets1: Vec<(u32, f32)> = g
            .out_neighbors_with_values(1)
            .map(|t| (t.target, t.value))
            .collect();
        assert_eq!(targets1, vec![(2, 2.0)]);
    }

    #[test]
    fn page_rank_triangle() {
        let g = triangle_graph();
        let config = PageRankConfig::new(100, 1e-6, 0.85);
        let (scores, iters, error) = page_rank(&g, config);
        assert_eq!(scores.len(), 3);
        assert!((scores[0] - scores[1]).abs() < 1e-4);
        assert!((scores[1] - scores[2]).abs() < 1e-4);
        assert!(error < 1e-6 || iters == 100);
    }

    #[test]
    fn page_rank_star() {
        let g = CsrBuilder::new().edges([(0, 1), (0, 2), (0, 3)]).build();
        let config = PageRankConfig::new(100, 1e-6, 0.85);
        let (scores, _, _) = page_rank(&g, config);
        assert!(scores[1] > scores[0]);
        assert!(scores[2] > scores[0]);
        assert!(scores[3] > scores[0]);
    }
}
