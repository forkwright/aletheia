//! PageRank over CSR graphs.

use super::DirectedCsrGraph;

#[derive(Copy, Clone, Debug)]
pub(crate) struct PageRankConfig {
    pub max_iterations: usize,
    pub tolerance: f64,
    pub damping_factor: f32,
}

impl PageRankConfig {
    pub fn new(max_iterations: usize, tolerance: f64, damping_factor: f32) -> Self {
        Self {
            max_iterations,
            tolerance,
            damping_factor,
        }
    }
}

pub(crate) fn page_rank(
    graph: &DirectedCsrGraph,
    config: PageRankConfig,
) -> (Vec<f32>, usize, f64) {
    let PageRankConfig {
        max_iterations,
        tolerance,
        damping_factor,
    } = config;

    let node_count = graph.node_count() as usize;
    let init_score = 1_f32 / node_count as f32;
    let base_score = (1.0_f32 - damping_factor) / node_count as f32;

    let mut out_scores: Vec<f32> = (0..node_count)
        .map(|n| init_score / graph.out_degree(n as u32) as f32)
        .collect();

    let mut scores = vec![init_score; node_count];

    let mut iteration = 0;

    loop {
        let mut error = 0_f64;

        for u in 0..node_count {
            let incoming_total: f32 = graph
                .in_neighbors(u as u32)
                .map(|&v| out_scores[v as usize])
                .sum();

            let old_score = scores[u];
            let new_score = base_score + damping_factor * incoming_total;

            scores[u] = new_score;
            error += f64::from((new_score - old_score).abs());

            out_scores[u] = new_score / graph.out_degree(u as u32) as f32;
        }

        iteration += 1;

        if error < tolerance || iteration == max_iterations {
            return (scores, iteration, error);
        }
    }
}
