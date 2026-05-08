//! Group matches into buckets keyed by a metadata field. Terminal stage:
//! the executor wraps the result in `ExecutionResult::Grouped`.
//!
//! Behaviour matches the inline implementation that used to live in
//! `query_vectors`: insertion order preserved per bucket, `limit` caps the
//! number of buckets returned, `group_size` caps members per bucket. If matches
//! exist but none of them carry the requested field, the executor surfaces
//! `ApiError::groupby_field_missing` — that error is built outside this stage.

use std::collections::HashMap;

use crate::handlers::query::{GroupResult, Match};

/// Output of the group-by stage.
pub struct GroupByOutput {
    pub groups: Vec<GroupResult>,
    /// True when at least one input match carried the requested field.
    /// The caller uses this to decide whether to surface `groupby_field_missing`.
    pub field_seen: bool,
    /// Number of input matches (used for the same error-decision logic).
    pub total_input: usize,
}

pub fn run(matches: Vec<Match>, field: &str, limit: usize, group_size: usize) -> GroupByOutput {
    let total_input = matches.len();
    let mut field_seen = false;
    let mut group_order: Vec<String> = Vec::new();
    let mut groups_map: HashMap<String, Vec<Match>> = HashMap::new();

    for m in matches {
        let raw = m.metadata.as_ref().and_then(|meta| meta.get(field));
        let group_key = match raw {
            Some(v) => {
                field_seen = true;
                match v.as_str() {
                    Some(s) => s.to_string(),
                    None => v.to_string(),
                }
            }
            None => String::new(),
        };

        if !groups_map.contains_key(&group_key) {
            group_order.push(group_key.clone());
        }
        let entry = groups_map.entry(group_key).or_default();
        if entry.len() < group_size {
            entry.push(m);
        }
    }

    let groups: Vec<GroupResult> = group_order
        .into_iter()
        .take(limit)
        .map(|key| GroupResult {
            matches: groups_map.remove(&key).unwrap_or_default(),
            key,
        })
        .collect();

    GroupByOutput {
        groups,
        field_seen,
        total_input,
    }
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
    fn buckets_by_field_with_caps() {
        let out = run(
            vec![
                m("a", json!({"u": "x"})),
                m("b", json!({"u": "x"})),
                m("c", json!({"u": "x"})),
                m("d", json!({"u": "y"})),
            ],
            "u",
            10,
            2,
        );
        assert!(out.field_seen);
        assert_eq!(out.groups.len(), 2);
        let bucket_x = out.groups.iter().find(|g| g.key == "x").unwrap();
        assert_eq!(bucket_x.matches.len(), 2); // group_size cap
    }

    #[test]
    fn no_match_carries_field_signals_caller() {
        let out = run(vec![m("a", json!({"other": 1}))], "u", 10, 3);
        assert!(!out.field_seen);
        assert_eq!(out.total_input, 1);
    }

    #[test]
    fn empty_input_is_no_groups() {
        let out = run(vec![], "u", 10, 3);
        assert!(out.groups.is_empty());
        assert!(!out.field_seen);
    }
}
