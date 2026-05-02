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
                        "$gt" => {
                            if op_val.is_string() {
                                format!("({sql_field})::timestamptz > ${n}::timestamptz")
                            } else {
                                format!("({sql_field})::numeric > ${n}::numeric")
                            }
                        }
                        "$gte" => {
                            if op_val.is_string() {
                                format!("({sql_field})::timestamptz >= ${n}::timestamptz")
                            } else {
                                format!("({sql_field})::numeric >= ${n}::numeric")
                            }
                        }
                        "$lt" => {
                            if op_val.is_string() {
                                format!("({sql_field})::timestamptz < ${n}::timestamptz")
                            } else {
                                format!("({sql_field})::numeric < ${n}::numeric")
                            }
                        }
                        "$lte" => {
                            if op_val.is_string() {
                                format!("({sql_field})::timestamptz <= ${n}::timestamptz")
                            } else {
                                format!("({sql_field})::numeric <= ${n}::numeric")
                            }
                        }
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
                        "$geoRadius" => {
                            let obj = op_val.as_object().ok_or_else(|| {
                                FilterError::Malformed(
                                    "$geoRadius requires {lat, lon, radiusMeters}".to_string(),
                                )
                            })?;
                            let lat = obj.get("lat").and_then(|v| v.as_f64()).ok_or_else(|| {
                                FilterError::Malformed(
                                    "$geoRadius.lat must be a number".to_string(),
                                )
                            })?;
                            let lon = obj.get("lon").and_then(|v| v.as_f64()).ok_or_else(|| {
                                FilterError::Malformed(
                                    "$geoRadius.lon must be a number".to_string(),
                                )
                            })?;
                            let radius = obj
                                .get("radiusMeters")
                                .and_then(|v| v.as_f64())
                                .ok_or_else(|| {
                                    FilterError::Malformed(
                                        "$geoRadius.radiusMeters must be a number".to_string(),
                                    )
                                })?;
                            let obj_path = jsonb_object_path(field);
                            format!(
                                "earth_distance(\
                                    ll_to_earth(({obj_path}->>'lat')::float8, ({obj_path}->>'lon')::float8),\
                                    ll_to_earth({lat}, {lon})\
                                ) <= {radius}"
                            )
                        }
                        "$geoBBox" => {
                            let obj = op_val.as_object().ok_or_else(|| {
                                FilterError::Malformed(
                                    "$geoBBox requires {minLat, maxLat, minLon, maxLon}"
                                        .to_string(),
                                )
                            })?;
                            let min_lat =
                                obj.get("minLat").and_then(|v| v.as_f64()).ok_or_else(|| {
                                    FilterError::Malformed(
                                        "$geoBBox.minLat must be a number".to_string(),
                                    )
                                })?;
                            let max_lat =
                                obj.get("maxLat").and_then(|v| v.as_f64()).ok_or_else(|| {
                                    FilterError::Malformed(
                                        "$geoBBox.maxLat must be a number".to_string(),
                                    )
                                })?;
                            let min_lon =
                                obj.get("minLon").and_then(|v| v.as_f64()).ok_or_else(|| {
                                    FilterError::Malformed(
                                        "$geoBBox.minLon must be a number".to_string(),
                                    )
                                })?;
                            let max_lon =
                                obj.get("maxLon").and_then(|v| v.as_f64()).ok_or_else(|| {
                                    FilterError::Malformed(
                                        "$geoBBox.maxLon must be a number".to_string(),
                                    )
                                })?;
                            let obj_path = jsonb_object_path(field);
                            format!(
                                "(({obj_path}->>'lat')::float8 BETWEEN {min_lat} AND {max_lat} \
                                  AND ({obj_path}->>'lon')::float8 BETWEEN {min_lon} AND {max_lon})"
                            )
                        }
                        "$elemMatch" => {
                            if !op_val.is_object() {
                                return Err(FilterError::Malformed(
                                    "$elemMatch value must be a JSON object".to_string(),
                                ));
                            }
                            let match_json = serde_json::to_string(op_val).map_err(|e| {
                                FilterError::Malformed(format!(
                                    "$elemMatch serialization error: {e}"
                                ))
                            })?;
                            let escaped = match_json.replace('\'', "''");
                            let obj_path = jsonb_object_path(field);
                            format!("{obj_path} @> '[{escaped}]'::jsonb")
                        }
                        "$contains" => {
                            // Match arrays of scalars: metadata->'field' @> '[<value>]'::jsonb
                            if !is_filter_scalar(op_val) {
                                return Err(FilterError::Malformed(
                                    "$contains value must be a scalar (string, number, or boolean)"
                                        .to_string(),
                                ));
                            }
                            let value_json = serde_json::to_string(op_val).map_err(|e| {
                                FilterError::Malformed(format!(
                                    "$contains serialization error: {e}"
                                ))
                            })?;
                            let escaped = value_json.replace('\'', "''");
                            let obj_path = jsonb_object_path(field);
                            format!("{obj_path} @> '[{escaped}]'::jsonb")
                        }
                        "$containsAny" => {
                            let arr = op_val.as_array().ok_or_else(|| {
                                FilterError::Malformed(
                                    "$containsAny requires an array of scalars".to_string(),
                                )
                            })?;
                            if arr.is_empty() {
                                return Err(FilterError::Malformed(
                                    "$containsAny array must not be empty".to_string(),
                                ));
                            }
                            let obj_path = jsonb_object_path(field);
                            let mut clauses = Vec::with_capacity(arr.len());
                            for v in arr {
                                if !is_filter_scalar(v) {
                                    return Err(FilterError::Malformed(
                                        "$containsAny array elements must be scalars".to_string(),
                                    ));
                                }
                                let value_json = serde_json::to_string(v).map_err(|e| {
                                    FilterError::Malformed(format!(
                                        "$containsAny serialization error: {e}"
                                    ))
                                })?;
                                let escaped = value_json.replace('\'', "''");
                                clauses.push(format!("{obj_path} @> '[{escaped}]'::jsonb"));
                            }
                            format!("({})", clauses.join(" OR "))
                        }
                        "$containsAll" => {
                            let arr = op_val.as_array().ok_or_else(|| {
                                FilterError::Malformed(
                                    "$containsAll requires an array of scalars".to_string(),
                                )
                            })?;
                            if arr.is_empty() {
                                return Err(FilterError::Malformed(
                                    "$containsAll array must not be empty".to_string(),
                                ));
                            }
                            for v in arr {
                                if !is_filter_scalar(v) {
                                    return Err(FilterError::Malformed(
                                        "$containsAll array elements must be scalars".to_string(),
                                    ));
                                }
                            }
                            let array_json = serde_json::to_string(op_val).map_err(|e| {
                                FilterError::Malformed(format!(
                                    "$containsAll serialization error: {e}"
                                ))
                            })?;
                            let escaped = array_json.replace('\'', "''");
                            let obj_path = jsonb_object_path(field);
                            format!("{obj_path} @> '{escaped}'::jsonb")
                        }
                        other => return Err(FilterError::UnsupportedOperator(other.to_string())),
                    };

                    // $in and $nin embed values directly (safe because we quote them above)
                    // All other operators use parameterized binds
                    match op.as_str() {
                        "$in" | "$nin" | "$geoRadius" | "$geoBBox" | "$elemMatch" | "$contains"
                        | "$containsAny" | "$containsAll" => {} // values inlined in SQL above
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

/// Convert a field name to a JSONB object-accessor expression (uses -> throughout).
/// Unlike jsonb_field_accessor, the result is a JSONB value, not text.
/// Used when sub-field access or array containment is needed on the result.
///
/// "location"  -> "metadata->'location'"
/// "place.geo" -> "metadata->'place'->'geo'"
/// True for JSON values that are valid as `$contains`/`$containsAny`/`$containsAll`
/// elements: strings, numbers, booleans. Rejects null, arrays, and objects.
fn is_filter_scalar(v: &Value) -> bool {
    v.is_string() || v.is_number() || v.is_boolean()
}

pub(crate) fn jsonb_object_path(field: &str) -> String {
    let mut acc = "metadata".to_string();
    for part in field.split('.') {
        acc.push_str(&format!("->'{part}'"));
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

    // --- Datetime ---

    #[test]
    fn gte_string_uses_timestamptz() {
        let (sql, params) =
            translate_filter(&json!({"created_at": {"$gte": "2025-01-01T00:00:00Z"}}), 0).unwrap();
        assert!(
            sql.contains("::timestamptz >= $1::timestamptz"),
            "got: {sql}"
        );
        assert_eq!(params[0], json!("2025-01-01T00:00:00Z"));
    }

    #[test]
    fn gt_number_still_uses_numeric() {
        let (sql, _) = translate_filter(&json!({"score": {"$gt": 42}}), 0).unwrap();
        assert!(sql.contains("::numeric > $1"), "got: {sql}");
    }

    #[test]
    fn lte_string_uses_timestamptz() {
        let (sql, params) =
            translate_filter(&json!({"updated_at": {"$lte": "2025-12-31T23:59:59Z"}}), 0).unwrap();
        assert!(
            sql.contains("::timestamptz <= $1::timestamptz"),
            "got: {sql}"
        );
        assert_eq!(params.len(), 1);
    }

    // --- Geo ---

    #[test]
    fn geo_radius_sql() {
        let (sql, params) = translate_filter(
            &json!({"location": {"$geoRadius": {"lat": 40.7, "lon": -74.0, "radiusMeters": 5000.0}}}),
            0,
        )
        .unwrap();
        assert!(
            sql.contains("earth_distance") && sql.contains("ll_to_earth"),
            "got: {sql}"
        );
        assert!(sql.contains("<= 5000"), "got: {sql}");
        assert!(params.is_empty());
    }

    #[test]
    fn geo_radius_nested_field() {
        let (sql, _) = translate_filter(
            &json!({"place.coords": {"$geoRadius": {"lat": 1.0, "lon": 2.0, "radiusMeters": 100.0}}}),
            0,
        )
        .unwrap();
        assert!(sql.contains("metadata->'place'->'coords'"), "got: {sql}");
    }

    #[test]
    fn geo_bbox_sql() {
        let (sql, params) = translate_filter(
            &json!({"location": {"$geoBBox": {"minLat": 40.0, "maxLat": 41.0, "minLon": -75.0, "maxLon": -73.0}}}),
            0,
        )
        .unwrap();
        assert!(
            sql.contains("BETWEEN 40") && sql.contains("BETWEEN -75"),
            "got: {sql}"
        );
        assert!(params.is_empty());
    }

    #[test]
    fn geo_radius_missing_field_errors() {
        // missing radiusMeters
        let r = translate_filter(
            &json!({"location": {"$geoRadius": {"lat": 40.7, "lon": -74.0}}}),
            0,
        );
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn geo_radius_non_object_errors() {
        let r = translate_filter(&json!({"location": {"$geoRadius": "not-an-object"}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    // --- $elemMatch ---

    #[test]
    fn elem_match_basic() {
        let (sql, params) =
            translate_filter(&json!({"tags": {"$elemMatch": {"type": "premium"}}}), 0).unwrap();
        assert!(sql.contains("@>") && sql.contains("::jsonb"), "got: {sql}");
        assert!(sql.contains("metadata->'tags'"), "got: {sql}");
        assert!(params.is_empty());
    }

    #[test]
    fn elem_match_non_object_errors() {
        let r = translate_filter(&json!({"tags": {"$elemMatch": "not-an-object"}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn elem_match_single_quote_escaped() {
        let (sql, _) =
            translate_filter(&json!({"tags": {"$elemMatch": {"name": "O'Brien"}}}), 0).unwrap();
        assert!(
            sql.contains("O''Brien"),
            "single quote must be escaped, got: {sql}"
        );
    }

    #[test]
    fn elem_match_nested_field() {
        let (sql, _) = translate_filter(
            &json!({"user.roles": {"$elemMatch": {"level": "admin"}}}),
            0,
        )
        .unwrap();
        assert!(sql.contains("metadata->'user'->'roles'"), "got: {sql}");
    }

    #[test]
    fn elem_match_does_not_perturb_param_offset() {
        let (sql, params) = translate_filter(
            &json!({"$and": [
                {"tags": {"$elemMatch": {"type": "premium"}}},
                {"score": {"$gte": 5}}
            ]}),
            2,
        )
        .unwrap();
        // $gte 5 is numeric, so uses ::numeric; param is at offset 2+0+1 = $3
        assert!(sql.contains("$3"), "score param should be $3, got: {sql}");
        assert_eq!(params.len(), 1);
    }

    // --- $contains / $containsAny / $containsAll ---

    #[test]
    fn contains_string_basic() {
        let (sql, params) =
            translate_filter(&json!({"authors": {"$contains": "Cortex Team"}}), 0).unwrap();
        assert!(
            sql.contains("metadata->'authors'") && sql.contains("@>"),
            "got: {sql}"
        );
        assert!(sql.contains(r#"["Cortex Team"]"#), "got: {sql}");
        assert!(params.is_empty());
    }

    #[test]
    fn contains_numeric() {
        let (sql, params) = translate_filter(&json!({"ratings": {"$contains": 5}}), 0).unwrap();
        assert!(sql.contains("[5]"), "got: {sql}");
        assert!(params.is_empty());
    }

    #[test]
    fn contains_bool() {
        let (sql, _) = translate_filter(&json!({"flags": {"$contains": true}}), 0).unwrap();
        assert!(sql.contains("[true]"), "got: {sql}");
    }

    #[test]
    fn contains_rejects_object_element() {
        let r = translate_filter(&json!({"tags": {"$contains": {"a": 1}}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_rejects_array_element() {
        let r = translate_filter(&json!({"tags": {"$contains": ["a"]}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_rejects_null() {
        let r = translate_filter(&json!({"tags": {"$contains": null}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_single_quote_escaped() {
        let (sql, _) = translate_filter(&json!({"authors": {"$contains": "O'Brien"}}), 0).unwrap();
        assert!(
            sql.contains("O''Brien"),
            "single quote must be escaped, got: {sql}"
        );
    }

    #[test]
    fn contains_nested_field() {
        let (sql, _) = translate_filter(&json!({"meta.tags": {"$contains": "rag"}}), 0).unwrap();
        assert!(sql.contains("metadata->'meta'->'tags'"), "got: {sql}");
    }

    #[test]
    fn contains_any_basic() {
        let (sql, params) = translate_filter(
            &json!({"authors": {"$containsAny": ["Cortex Team", "Lewis"]}}),
            0,
        )
        .unwrap();
        assert!(sql.contains(r#"["Cortex Team"]"#) && sql.contains(r#"["Lewis"]"#));
        assert!(sql.contains(" OR "), "got: {sql}");
        assert!(params.is_empty());
    }

    #[test]
    fn contains_any_empty_errors() {
        let r = translate_filter(&json!({"tags": {"$containsAny": []}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_any_non_array_errors() {
        let r = translate_filter(&json!({"tags": {"$containsAny": "x"}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_any_rejects_non_scalar_element() {
        let r = translate_filter(&json!({"tags": {"$containsAny": ["x", {"a": 1}]}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_all_basic() {
        let (sql, params) = translate_filter(
            &json!({"authors": {"$containsAll": ["Smith", "Johnson"]}}),
            0,
        )
        .unwrap();
        // Single containment check with the full array
        assert!(sql.contains(r#"["Smith","Johnson"]"#), "got: {sql}");
        assert!(
            !sql.contains(" OR "),
            "containsAll is one check, got: {sql}"
        );
        assert!(params.is_empty());
    }

    #[test]
    fn contains_all_empty_errors() {
        let r = translate_filter(&json!({"tags": {"$containsAll": []}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_all_rejects_non_scalar_element() {
        let r = translate_filter(&json!({"tags": {"$containsAll": ["x", ["nested"]]}}), 0);
        assert!(matches!(r, Err(FilterError::Malformed(_))));
    }

    #[test]
    fn contains_does_not_perturb_param_offset() {
        let (sql, params) = translate_filter(
            &json!({"$and": [
                {"authors": {"$contains": "Cortex Team"}},
                {"score": {"$gte": 5}}
            ]}),
            2,
        )
        .unwrap();
        // $gte 5 is numeric; param should be at $3 (offset 2 + 0 inlined + 1)
        assert!(sql.contains("$3"), "score param should be $3, got: {sql}");
        assert_eq!(params.len(), 1);
    }

    // --- jsonb_object_path helper ---

    #[test]
    fn jsonb_object_path_single() {
        assert_eq!(jsonb_object_path("location"), "metadata->'location'");
    }

    #[test]
    fn jsonb_object_path_nested() {
        assert_eq!(jsonb_object_path("place.geo"), "metadata->'place'->'geo'");
    }
}
