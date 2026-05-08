use crate::handlers::query::Match;

pub fn run(mut matches: Vec<Match>, k: i64) -> Vec<Match> {
    if k < 0 {
        return Vec::new();
    }
    matches.truncate(k as usize);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str) -> Match {
        Match {
            id: id.into(),
            score: 0.0,
            values: None,
            metadata: None,
        }
    }

    #[test]
    fn truncates_to_k() {
        let out = run(vec![m("a"), m("b"), m("c")], 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[1].id, "b");
    }

    #[test]
    fn k_larger_than_input_is_noop() {
        assert_eq!(run(vec![m("a")], 5).len(), 1);
    }
}
