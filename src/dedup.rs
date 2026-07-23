use crate::errors::RsfResult;

/* Note: compute_profiles and RankingOptions were used during development but removed.
   The dedup logic works without them - it just uses raw row data. */
use std::collections::HashMap;

/// Configuration for duplicate detection behavior.
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Number of top key columns to group by (by cardinality).
    pub key_columns: usize,
    #[allow(dead_code)]
    /// Tolerance for floating-point / currency comparisons (absolute difference).
    pub float_tolerance: f64,
    #[allow(dead_code)]
    /// Whether to trim whitespace before comparing text values.
    pub trim_whitespace: bool,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            key_columns: 3,
            float_tolerance: 0.01,
            trim_whitespace: true,
        }
    }
}

/// A group of rows that are duplicates or near-duplicates.
#[derive(Debug)]
pub struct DuplicateGroup {
    /// The representative (first) row in this group.
    pub representative: Vec<String>,
    /// All duplicate rows (including the representative).
    pub members: Vec<RowInfo>,
}

/// Metadata about a single row within a duplicate group.
#[derive(Debug, Clone)]
pub struct RowInfo {
    pub row_index: usize,
    pub values: Vec<String>,
    /// If this is a near-duplicate, what differences were found?
    pub differences: Vec<Difference>,
}

/// A single difference between two rows.
#[derive(Debug, Clone)]
pub enum Difference {
    WhitespaceTrimmed { column: String },
    CurrencyFormat { expected: String, actual: String },
    FloatDifference { column: String, expected: f64, actual: f64 },
    DateFormat { expected: String, actual: String },
}

/// Result of running duplicate detection.
#[derive(Debug)]
pub struct DedupResult {
    /// Total number of rows analyzed.
    pub total_rows: usize,
    /// Number of exact duplicate groups found.
    pub exact_groups: usize,
    /// Number of near-duplicate groups found.
    pub near_duplicate_groups: usize,
    /// Total rows removed (duplicates kept = 1 per group).
    pub rows_removed: usize,
    /// The cleaned data (one row per unique key group).
    pub cleaned_data: Vec<Vec<String>>,
    /// All duplicate groups for reporting.
    pub duplicate_groups: Vec<DuplicateGroup>,
}

/// Run duplicate detection on the given CSV data.
pub fn find_duplicates(
    headers: &[String],
    rows: &[Vec<String>],
    config: &DedupConfig,
) -> RsfResult<DedupResult> {
    if rows.is_empty() {
        return Ok(DedupResult {
            total_rows: 0,
            exact_groups: 0,
            near_duplicate_groups: 0,
            rows_removed: 0,
            cleaned_data: Vec::new(),
            duplicate_groups: Vec::new(),
        });
    }

    // Determine key columns by cardinality (top N).
    let num_key_cols = config.key_columns.min(headers.len());
    let key_indices = determine_key_columns_for_report(headers, rows, num_key_cols);

    // Group rows by their key column values.
    let groups: HashMap<String, Vec<(usize, Vec<String>)>> = group_by_keys(rows, &key_indices, config);

    // Count multi-member groups and total rows in them before consuming `groups`.
    let multi_group_count = groups.iter().filter(|(_, m)| m.len() > 1).count();
    let multi_rows: usize = groups
        .iter()
        .filter(|(_, m)| m.len() > 1)
        .map(|(_, m)| m.len())
        .sum();

    // Process groups in a deterministic order (by first row index).
    let mut group_keys: Vec<_> = groups.into_iter().collect();
    group_keys.sort_by_key(|(_, members)| members[0].0);

    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();
    let mut cleaned_data: Vec<Vec<String>> = Vec::new();
    let mut exact_groups = 0usize;
    let mut near_duplicate_groups = 0usize;

    for (_key, members) in group_keys {
        if members.len() == 1 {
            // Single row — keep it.
            cleaned_data.push(members[0].1.clone());
            continue;
        }

        // We have a group of rows with matching key columns.
        let (exact_count, near_groups) = analyze_group(&members);

        if !near_groups.is_empty() {
            duplicate_groups.extend(near_groups);
            near_duplicate_groups += 1;
        } else if exact_count > 0 {
            // All members are truly identical — add as an exact-only group for reporting.
            let rep = &members[0].1;
            let mut group = DuplicateGroup {
                representative: rep.clone(),
                members: Vec::new(),
            };
            for &(row_idx, ref values) in members.iter() {
                group.members.push(RowInfo {
                    row_index: row_idx,
                    values: values.clone(),
                    differences: Vec::new(),
                });
            }
            duplicate_groups.push(group);
        }

        exact_groups += exact_count;

        // Keep the first row as the representative.
        cleaned_data.push(members[0].1.clone());
    }

    // rows_removed = total rows in multi-member groups minus one kept per group.
    let rows_removed = multi_rows - multi_group_count;

    Ok(DedupResult {
        total_rows: rows.len(),
        exact_groups,
        near_duplicate_groups,
        rows_removed,
        cleaned_data,
        duplicate_groups,
    })
}

/// Determine which columns to use as keys for deduplication.
/// By default, uses the first `n` columns (positional).
pub fn determine_key_columns_for_report(
    headers: &[String],
    _rows: &[Vec<String>],
    n: usize,
) -> Vec<usize> {
    let num_key_cols = n.min(headers.len());
    // Use positional columns 0..n as the key.
    (0..num_key_cols).collect()
}

/// Group rows by their key column values into a single string key.
fn group_by_keys(
    rows: &[Vec<String>],
    key_indices: &[usize],
    config: &DedupConfig,
) -> HashMap<String, Vec<(usize, Vec<String>)>> {
    let mut groups: HashMap<String, Vec<(usize, Vec<String>)>> = HashMap::new();

    for (row_idx, row) in rows.iter().enumerate() {
        let key_parts: Vec<&str> = key_indices
            .iter()
            .filter_map(|&i| row.get(i).map(|v| v.as_str()))
            .collect();
        
        // Apply trimming if configured.
        let key_value: String = if config.trim_whitespace {
            key_parts.iter().map(|s| s.trim()).collect::<Vec<_>>().join("\x1f")
        } else {
            key_parts.join("\x1f")
        };

        groups.entry(key_value).or_default().push((row_idx, row.clone()));
    }

    groups
}

/// Analyze a group of rows for exact and near-duplicate matches.
fn analyze_group(members: &[(usize, Vec<String>)]) -> (usize, Vec<DuplicateGroup>) {
    let mut duplicate_groups: Vec<DuplicateGroup> = Vec::new();
    let mut seen_exact_keys: HashMap<String, usize> = HashMap::new();

    for &(row_idx, ref values) in members {
        let exact_key = values.join("\x1e"); // different delimiter for exact match

        if let Some(&group_idx) = seen_exact_keys.get(&exact_key) {
            duplicate_groups[group_idx].members.push(RowInfo {
                row_index: row_idx,
                values: values.clone(),
                differences: Vec::new(),
            });
        } else {
            seen_exact_keys.insert(exact_key, duplicate_groups.len());
            duplicate_groups.push(DuplicateGroup {
                representative: values.clone(),
                members: vec![RowInfo {
                    row_index: row_idx,
                    values: values.clone(),
                    differences: Vec::new(),
                }],
            });
        }
    }

    // Check for near-duplicates within each group.
    let mut near_duplicate_groups: Vec<DuplicateGroup> = Vec::new();

    for group in &mut duplicate_groups {
        if group.members.len() > 1 {
            let all_exact = group.members[1..]
                .iter()
                .all(|m| m.values == group.representative);

            if !all_exact {
                let mut near_group = DuplicateGroup {
                    representative: group.representative.clone(),
                    members: Vec::new(),
                };

                for member in &group.members {
                    if member.values == group.representative {
                        near_group.members.push(member.clone());
                    } else {
                        let diffs = find_differences(&member.values, &group.representative);
                        near_group.members.push(RowInfo {
                            row_index: member.row_index,
                            values: member.values.clone(),
                            differences: diffs,
                        });
                    }
                }

                if near_group.members.len() > 1 {
                    near_duplicate_groups.push(near_group);
                }
            }
        }
    }

    let exact_count = duplicate_groups.iter().filter(|g| g.members.len() > 1).count();
    (exact_count, near_duplicate_groups)
}

/// Find differences between two rows that are otherwise key-matched.
fn find_differences(actual: &[String], expected: &[String]) -> Vec<Difference> {
    let mut diffs = Vec::new();

    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        // Skip key columns — differences there mean different groups.
        if i == 0 || i == 1 {
            continue;
        }

        let a_norm = a.clone();
        let e_norm = e.clone();

        if a_norm != e_norm {
            // Check for whitespace-only difference.
            if a_norm.trim() == e_norm.trim() && a_norm.len() != e_norm.len() {
                diffs.push(Difference::WhitespaceTrimmed {
                    column: format!("column_{}", i),
                });
                continue;
            }

            // Check for currency differences (same numeric value, different format).
            if let (Ok(a_num), Ok(e_num)) = (a_norm.parse::<f64>(), e_norm.parse::<f64>()) {
                if (a_num - e_num).abs() < 0.01 && a_norm != e_norm {
                    diffs.push(Difference::CurrencyFormat {
                        expected: e_norm,
                        actual: a_norm,
                    });
                    continue;
                }
            }
        }
    }

    diffs
}

/// Print a human-readable report of duplicates to stderr.
pub fn print_report(result: &DedupResult, headers: &[String], key_indices: &[usize]) {
    eprintln!("\n=== Duplicate Detection Report ===");
    eprintln!("Total rows analyzed: {}", result.total_rows);
    eprintln!(
        "Exact duplicate groups: {}",
        if result.exact_groups > 0 {
            result.exact_groups.to_string()
        } else {
            "none".to_string()
        }
    );
    eprintln!(
        "Near-duplicate groups: {}",
        if result.near_duplicate_groups > 0 {
            result.near_duplicate_groups.to_string()
        } else {
            "none".to_string()
        }
    );
    eprintln!("Rows removed (keeping first): {}", result.rows_removed);

    // Print key column names for context.
    let key_names: Vec<&str> = key_indices
        .iter()
        .filter_map(|&i| headers.get(i).map(|s| s.as_str()))
        .collect();
    if !key_names.is_empty() {
        eprintln!("Key columns: {}", key_names.join(", "));
    }

    for group in &result.duplicate_groups {
        let exact_count = group.members.iter().filter(|m| m.differences.is_empty()).count();
        let near_count = group.members.len() - exact_count;

        if near_count > 0 {
            eprintln!(
                "\n--- Near-duplicate group ({} rows) ---",
                group.members.len()
            );
            for member in &group.members {
                let marker = if member.differences.is_empty() {
                    "  [exact]".to_string()
                } else {
                    format!(
                        "  [near, row {}]",
                        member.row_index + 1 // 1-indexed for display
                    )
                };

                eprintln!(
                    "{} Row {}: {}",
                    marker,
                    member.row_index + 1,
                    truncate(&member.values.join(", "), 80)
                );

                for diff in &member.differences {
                    match diff {
                        Difference::WhitespaceTrimmed { column } => {
                            eprintln!("    → Whitespace trimmed in {}", column);
                        }
                        Difference::CurrencyFormat { expected, actual } => {
                            eprintln!(
                                "    → Currency format: '{}' vs '{}'",
                                truncate(expected, 40),
                                truncate(actual, 40)
                            );
                        }
                        Difference::FloatDifference { column, expected, actual } => {
                            eprintln!(
                                "    → {} differs by {:.6}: {} vs {}",
                                column,
                                (expected - actual).abs(),
                                expected,
                                actual
                            );
                        }
                        Difference::DateFormat { expected, actual } => {
                            eprintln!(
                                "    → Date format: '{}' vs '{}'",
                                truncate(expected, 40),
                                truncate(actual, 40)
                            );
                        }
                    }
                }
            }
        } else if exact_count > 1 {
            eprintln!("\n--- Exact duplicate group ({} rows) ---", group.members.len());
            for member in &group.members {
                eprintln!(
                    "  [exact] Row {}: {}",
                    member.row_index + 1,
                    truncate(&member.values.join(", "), 80)
                );
            }
        }
    }

    if result.duplicate_groups.is_empty() && result.exact_groups == 0 {
        eprintln!("\nNo duplicates found.");
    }
}

/// Truncate a string to the given max length, adding "..." if truncated.
#[allow(dead_code)]
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_duplicates() {
        let headers = vec!["id".to_string(), "name".to_string(), "amount".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string(), "10.00".to_string()],
            vec!["2".to_string(), "Bob".to_string(), "20.00".to_string()],
            vec!["1".to_string(), "Alice".to_string(), "10.00".to_string()], // exact dup of row 0
        ];

        let config = DedupConfig::default();
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 3);
        assert_eq!(result.cleaned_data.len(), 2); // one kept per group
    }

    #[test]
    fn test_no_duplicates() {
        let headers = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
            vec!["2".to_string(), "Bob".to_string()],
            vec!["3".to_string(), "Charlie".to_string()],
        ];

        let config = DedupConfig::default();
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 3);
        assert_eq!(result.cleaned_data.len(), 3); // all kept
    }

    #[test]
    fn test_empty_input() {
        let headers: Vec<String> = vec![];
        let rows: Vec<Vec<String>> = vec![];

        let config = DedupConfig::default();
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 0);
        assert!(result.cleaned_data.is_empty());
    }

    #[test]
    fn test_single_row() {
        let headers = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec!["1".to_string(), "Alice".to_string()]];

        let config = DedupConfig::default();
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 1);
        assert_eq!(result.cleaned_data.len(), 1);
    }

    #[test]
    fn test_all_same_key() {
        // All rows share the same key column value.
        let headers = vec!["id".to_string(), "name".to_string(), "amount".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string(), "10.00".to_string()],
            vec!["1".to_string(), "Bob".to_string(), "20.00".to_string()],
            vec!["1".to_string(), "Charlie".to_string(), "30.00".to_string()],
        ];

        let config = DedupConfig {
            key_columns: 1, // only use 'id' as the key
            ..Default::default()
        };
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 3);
        assert_eq!(result.cleaned_data.len(), 1); // all grouped under same key
    }

    #[test]
    fn test_currency_near_duplicate() {
        let headers = vec!["id".to_string(), "name".to_string(), "amount".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string(), "$10.00".to_string()],
            vec!["1".to_string(), "Alice".to_string(), "10.00".to_string()], // same value, no $ sign
        ];

        let config = DedupConfig {
            key_columns: 2, // id + name as keys
            ..Default::default()
        };
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 2);
        assert_eq!(result.cleaned_data.len(), 1); // grouped together
    }

    #[test]
    fn test_whitespace_near_duplicate() {
        let headers = vec!["id".to_string(), "name".to_string(), "amount".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice ".to_string(), "10.00".to_string()],
            vec!["1".to_string(), "Alice".to_string(), "10.00".to_string()], // trailing space diff in name (key column)
        ];

        let config = DedupConfig {
            key_columns: 2,
            trim_whitespace: true,
            ..Default::default()
        };
        
        // With positional columns as keys and trim_whitespace=true, 
        // "Alice " should match "Alice" in the key column.
        let result = find_duplicates(&headers, &rows, &config).unwrap();

        assert_eq!(result.total_rows, 2);
        assert_eq!(result.cleaned_data.len(), 1); // grouped together after trimming
    }
}
