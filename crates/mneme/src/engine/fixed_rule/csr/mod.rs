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
        (self.offsets.len() - 1) as u32
    }

    fn degree(&self, node: u32) -> u32 {
        let i = node as usize;
        self.offsets[i + 1] - self.offsets[i]
    }

    fn targets_with_values(&self, node: u32) -> &[Target<EV>] {
        let i = node as usize;
        let from = self.offsets[i] as usize;
        let to = self.offsets[i + 1] as usize;
        &self.targets[from..to]
    }
}

impl Csr<()> {
    fn targets(&self, node: u32) -> &[u32] {
        let i = node as usize;
        let from = self.offsets[i] as usize;
        let to = self.offsets[i + 1] as usize;
        let slice = &self.targets[from..to];
        // Target<()> has the same layout as u32 due to #[repr(C)] and ()
        // being zero-sized. This matches the original graph_builder behavior.
        debug_assert_eq!(
            std::mem::size_of::<Target<()>>(),
            std::mem::size_of::<u32>()
        );
        // SAFETY: Target<()> is #[repr(C)] with a u32 field and a ZST,
        // so it has identical size and alignment to u32.
        #[expect(unsafe_code, reason = "CSR layout cast matching original graph crate")]
        unsafe {
            std::slice::from_raw_parts(slice.as_ptr().cast::<u32>(), slice.len())
        }
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
    pub fn out_neighbors(&self, node: u32) -> std::slice::Iter<'_, u32> {
        self.csr_out.targets(node).iter()
    }

    pub fn in_neighbors(&self, node: u32) -> std::slice::Iter<'_, u32> {
        self.csr_inc.targets(node).iter()
    }
}

pub(crate) struct CsrBuilder<EV> {
    edges: Vec<(u32, u32, EV)>,
    sorted: bool,
}

impl<EV: Copy> CsrBuilder<EV> {
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            sorted: false,
        }
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
            offsets.push(targets.len() as u32);
            targets.extend(list);
        }
        offsets.push(targets.len() as u32);

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
