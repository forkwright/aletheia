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

    #[expect(clippy::cast_sign_loss, reason = "graph node u32 fits usize")]
    let node_count = graph.node_count() as usize;
    #[expect(
        clippy::cast_precision_loss,
        reason = "node count acceptable as approximate float for scoring"
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "intentional f64 to f32 reduction"
    )]
    let node_count_f32 = node_count as f32;
    let init_score = 1_f32 / node_count_f32;
    let base_score = (1.0_f32 - damping_factor) / node_count_f32;

    let mut out_scores: Vec<f32> = (0..node_count)
        .map(|n| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "graph node count bounded by u32"
            )]
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let n_u32 = n as u32;
            #[expect(
                clippy::cast_precision_loss,
                reason = "out-degree acceptable as approximate float"
            )]
            #[expect(
                clippy::cast_possible_truncation,
                reason = "intentional f64 to f32 reduction"
            )]
            let degree_f32 = graph.out_degree(n_u32) as f32;
            init_score / degree_f32
        })
        .collect();

    let mut scores = vec![init_score; node_count];

    let mut iteration = 0;

    loop {
        let mut error = 0_f64;

        for u in 0..node_count {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "graph node count bounded by u32"
            )]
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let u_u32 = u as u32;
            let incoming_total: f32 = graph
                .in_neighbors(u_u32)
                .map(|v| out_scores[v as usize])
                .sum();

            #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
            let old_score = scores[u];
            let new_score = base_score + damping_factor * incoming_total;

            scores[u] = new_score;
            error += f64::from((new_score - old_score).abs());

            #[expect(
                clippy::cast_precision_loss,
                reason = "out-degree acceptable as approximate float"
            )]
            #[expect(
                clippy::cast_possible_truncation,
                reason = "intentional f64 to f32 reduction"
            )]
            let degree_f32 = graph.out_degree(u_u32) as f32;
            out_scores[u] = new_score / degree_f32;
        }

        iteration += 1;

        if error < tolerance || iteration == max_iterations {
            return (scores, iteration, error);
        }
    }
}
