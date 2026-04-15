//! `PageRank` power iteration over CSR graphs.
//!
//! Reference: Page, L. et al. (1999). "The `PageRank` Citation Ranking:
//! Bringing Order to the Web." Stanford technical report.

use super::DirectedCsrGraph;

/// Configuration for the `PageRank` power-iteration algorithm.
#[derive(Copy, Clone, Debug)]
#[must_use]
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

/// Compute `PageRank` scores via power iteration.
///
/// Returns `(scores, iterations_run, final_error)`.
///
/// **Complexity:** O(I * (V + E)) per iteration where I is the number of
/// iterations until convergence or the max iteration limit.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "PageRank score array indices are bounds-checked by the graph node count"
)]
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
    #[expect(
        clippy::cast_precision_loss,
        reason = "node count acceptable as approximate float for scoring"
    )]
    let node_count_f32 = node_count as f32;
    let initial_score = 1_f32 / node_count_f32;
    let base_score = (1.0_f32 - damping_factor) / node_count_f32;

    let mut out_scores: Vec<f32> = (0..node_count)
        .map(|node| {
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let node_u32 = node as u32;
            #[expect(
                clippy::cast_precision_loss,
                reason = "out-degree acceptable as approximate float"
            )]
            let degree_f32 = graph.out_degree(node_u32) as f32;
            initial_score / degree_f32
        })
        .collect();

    let mut scores = vec![initial_score; node_count];

    let mut iteration = 0;

    loop {
        let mut error = 0_f64;

        for node in 0..node_count {
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let node_u32 = node as u32;
            let incoming_total: f32 = graph
                .in_neighbors(node_u32)
                .map(|source| out_scores[source as usize])
                .sum();

            let old_score = scores[node];
            let new_score = base_score + damping_factor * incoming_total;

            scores[node] = new_score;
            error += f64::from((new_score - old_score).abs());

            #[expect(
                clippy::cast_precision_loss,
                reason = "out-degree acceptable as approximate float"
            )]
            let degree_f32 = graph.out_degree(node_u32) as f32;
            out_scores[node] = new_score / degree_f32;
        }

        iteration += 1;

        if error < tolerance || iteration == max_iterations {
            return (scores, iteration, error);
        }
    }
}
