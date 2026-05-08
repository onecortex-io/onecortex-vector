//! First-occurrence-wins dedupe by a metadata field.
//!
//! Matches without the field, or whose metadata is missing entirely, are kept
//! (we never silently drop rows because of missing metadata — that mirrors
//! the GroupBy precedent).

use std::collections::HashSet;

use crate::handlers::query::Match;

pub fn run(matches: Vec<Match>, field: &str) -> Vec<Match> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Match> = Vec::with_capacity(matches.len());
    for m in matches {
        let key = m
            .metadata
            .as_ref()
            .and_then(|meta| meta.get(field))
            .map(|v| match v.as_str() {
                Some(s) => s.to_string(),
                None => v.to_string(),
            });
        match key {
            Some(k) => {
                if seen.insert(k) {
                    out.push(m);
                }
            }
            None => out.push(m),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(id: &str, meta: serde_json::Value) -> Match {
        Match {
            id: id.into(),
            score: 0.0,
            values: None,
            metadata: Some(meta),
        }
    }

    #[test]
    fn first_occurrence_wins() {
        let out = run(
            vec![
                m("a", json!({"url": "x"})),
                m("b", json!({"url": "y"})),
                m("c", json!({"url": "x"})),
            ],
            "url",
        );
        assert_eq!(
            out.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn missing_field_keeps_all() {
        let out = run(vec![m("a", json!({})), m("b", json!({}))], "url");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn numeric_keys_handled() {
        let out = run(
            vec![
                m("a", json!({"n": 1})),
                m("b", json!({"n": 2})),
                m("c", json!({"n": 1})),
            ],
            "n",
        );
        assert_eq!(out.len(), 2);
    }
}
