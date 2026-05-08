use crate::handlers::query::Match;

/// Drop matches whose `score` is strictly below `min`. Equal-to-min is kept,
/// matching the existing inline behaviour (`m.score >= threshold`).
pub fn run(mut matches: Vec<Match>, min: f64) -> Vec<Match> {
    matches.retain(|m| m.score >= min);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(score: f64) -> Match {
        Match {
            id: format!("{score}"),
            score,
            values: None,
            metadata: None,
        }
    }

    #[test]
    fn drops_below_min_keeps_equal() {
        let out = run(vec![m(0.9), m(0.6), m(0.5), m(0.1)], 0.6);
        assert_eq!(
            out.iter().map(|m| m.score).collect::<Vec<_>>(),
            vec![0.9, 0.6]
        );
    }

    #[test]
    fn empty_input_safe() {
        assert!(run(vec![], 0.5).is_empty());
    }
}
