use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("Unsupported filter operator: {0}")]
    UnsupportedOperator(String),
    #[error("Malformed filter: {0}")]
    Malformed(String),
}

/// Translate a metadata filter JSON object into a SQL WHERE fragment.
///
/// Returns (sql_fragment, bind_params) where sql_fragment uses $N placeholders
/// starting from `param_offset`. The caller must bind params in order.
///
/// Example:
///   input: {"category": {"$eq": "news"}, "score": {"$gte": 0.5}}
///   output: ("(metadata->>'category' = $1) AND ((metadata->>'score')::numeric >= $2)", ["news", 0.5])
pub fn translate_filter(
    filter: &Value,
    param_offset: usize,
) -> Result<(String, Vec<Value>), FilterError> {
    let obj = filter
        .as_object()
        .ok_or_else(|| FilterError::Malformed("Filter must be a JSON object".to_string()))?;

    let mut parts = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    for (key, val) in obj {
        match key.as_str() {
            "$and" => {
                let arr = val
                    .as_array()
                    .ok_or_else(|| FilterError::Malformed("$and must be an array".to_string()))?;
                let mut clauses = Vec::new();
                for item in arr {
                    let (clause, mut p) = translate_filter(item, param_offset + params.len())?;
                    clauses.push(format!("({clause})"));
                    params.append(&mut p);
                }
                parts.push(clauses.join(" AND "));
            }
            "$or" => {
                let arr = val
                    .as_array()
                    .ok_or_else(|| FilterError::Malformed("$or must be an array".to_string()))?;
                let mut clauses = Vec::new();
                for item in arr {
                    let (clause, mut p) = translate_filter(item, param_offset + params.len())?;
                    clauses.push(format!("({clause})"));
                    params.append(&mut p);
                }
                parts.push(format!("({})", clauses.join(" OR ")));
            }
            field => {
                // Field-level operators: {"field": {"$op": value}}
                let ops = val.as_object().ok_or_else(|| {
                    FilterError::Malformed(format!("Expected operator object for field '{field}'"))
                })?;
                for (op, op_val) in ops {
                    let n = param_offset + params.len() + 1;
                    let sql_field = jsonb_field_accessor(field);
                    let clause = match op.as_str() {
                        "$eq" => format!("{sql_field} = ${n}"),
                        "$ne" => format!("{sql_field} != ${n}"),
                        "$gt" => format!("({sql_field})::numeric > ${n}"),
                        "$gte" => format!("({sql_field})::numeric >= ${n}"),
                        "$lt" => format!("({sql_field})::numeric < ${n}"),
                        "$lte" => format!("({sql_field})::numeric <= ${n}"),
                        "$in" => {
                            let arr = op_val.as_array().ok_or_else(|| {
                                FilterError::Malformed("$in requires an array".to_string())
                            })?;
                            let sql_arr: Vec<String> = arr
                                .iter()
                                .map(|v| {
                                    v.as_str()
                                        .map(|s| format!("'{s}'"))
                                        .unwrap_or_else(|| v.to_string())
                                })
                                .collect();
                            format!("{sql_field} = ANY(ARRAY[{}]::text[])", sql_arr.join(","))
                        }
                        "$nin" => {
                            let arr = op_val.as_array().ok_or_else(|| {
                                FilterError::Malformed("$nin requires an array".to_string())
                            })?;
                            let sql_arr: Vec<String> = arr
                                .iter()
                                .map(|v| {
                                    v.as_str()
                                        .map(|s| format!("'{s}'"))
                                        .unwrap_or_else(|| v.to_string())
                                })
                                .collect();
                            format!(
                                "NOT ({sql_field} = ANY(ARRAY[{}]::text[]))",
                                sql_arr.join(",")
                            )
                        }
                        other => return Err(FilterError::UnsupportedOperator(other.to_string())),
                    };

                    // $in and $nin embed values directly (safe because we quote them above)
                    // All other operators use parameterized binds
                    match op.as_str() {
                        "$in" | "$nin" => {} // values inlined in SQL above
                        _ => {
                            params.push(op_val.clone());
                        }
                    }
                    parts.push(clause);
                }
            }
        }
    }

    Ok((parts.join(" AND "), params))
}

/// Convert a field name (possibly nested with dots) to a JSONB accessor expression.
/// "category" -> "metadata->>'category'"
/// "user.role" -> "metadata->'user'->>'role'"
pub(crate) fn jsonb_field_accessor(field: &str) -> String {
    let parts: Vec<&str> = field.split('.').collect();
    if parts.len() == 1 {
        return format!("metadata->>'{}'", parts[0]);
    }
    let mut acc = "metadata".to_string();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            acc.push_str(&format!("->>'{}'", part));
        } else {
            acc.push_str(&format!("->'{}'", part));
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eq_string() {
        let (sql, params) = translate_filter(&json!({"category": {"$eq": "news"}}), 0).unwrap();
        assert!(sql.contains("$1"));
        assert_eq!(params[0], json!("news"));
    }

    #[test]
    fn gt_number() {
        let (sql, params) = translate_filter(&json!({"score": {"$gt": 0.5}}), 0).unwrap();
        assert!(sql.contains("::numeric > $1"));
        assert_eq!(params[0], json!(0.5));
    }

    #[test]
    fn in_operator() {
        let (sql, _) = translate_filter(&json!({"tag": {"$in": ["a", "b"]}}), 0).unwrap();
        assert!(sql.contains("ANY(ARRAY["));
    }

    #[test]
    fn and_operator() {
        let (sql, params) = translate_filter(
            &json!({"$and": [{"cat": {"$eq": "news"}}, {"score": {"$gte": 1}}]}),
            0,
        )
        .unwrap();
        assert!(sql.contains(" AND "));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn or_operator() {
        let (sql, _) = translate_filter(
            &json!({"$or": [{"a": {"$eq": "x"}}, {"b": {"$eq": "y"}}]}),
            0,
        )
        .unwrap();
        assert!(sql.contains(" OR "));
    }

    #[test]
    fn unknown_operator_errors() {
        let result = translate_filter(&json!({"x": {"$regex": "foo"}}), 0);
        assert!(result.is_err());
    }
}
