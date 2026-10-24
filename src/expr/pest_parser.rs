//! Expression syntax validator for RSF table filtering.
//!
//! Validates expression syntax without building an AST.
//! The hand-written parser (parser.rs) handles actual AST construction.

/// Validate expression syntax using balanced-parenthesis checking and basic token validation.
/// Returns Ok(()) if valid, Err with description if invalid.
pub fn validate(input: &str) -> Result<(), String> {
    let input = input.trim();

    if input.is_empty() {
        return Err("Empty expression".to_string());
    }

    // Check balanced parentheses
    let mut depth = 0;
    for c in input.chars() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth < 0 {
                return Err("Unmatched closing parenthesis".to_string());
            }
        }
    }
    if depth != 0 {
        return Err("Unmatched opening parenthesis".to_string());
    }

    // Check balanced quotes for strings
    let mut in_string = false;
    for c in input.chars() {
        if c == '"' {
            in_string = !in_string;
        }
    }
    if in_string {
        return Err("Unterminated string literal".to_string());
    }

    // Check for trailing operator (common syntax error)
    let last_non_space = input.chars().rev().find(|c| !c.is_whitespace()).unwrap_or('\0');
    if matches!(last_non_space, '+' | '-' | '*' | '/' | '=' | '<' | '>' | '!') {
        return Err("Expression ends with operator".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_simple_comparison() {
        assert!(validate("amount > 100").is_ok());
    }

    #[test]
    fn test_validate_and_or() {
        assert!(validate("a AND b OR c").is_ok());
    }

    #[test]
    fn test_validate_parentheses() {
        assert!(validate("(a AND b) OR c").is_ok());
    }

    #[test]
    fn test_validate_in_clause() {
        assert!(validate("id IN (1, 2, 3)").is_ok());
    }

    #[test]
    fn test_validate_is_null() {
        assert!(validate("x IS NULL").is_ok());
    }

    #[test]
    fn test_validate_is_not_null() {
        assert!(validate("x IS NOT NULL").is_ok());
    }

    #[test]
    fn test_validate_arithmetic() {
        assert!(validate("a + b * c").is_ok());
    }

    #[test]
    fn test_validate_negative_number() {
        assert!(validate("a > -5").is_ok());
    }

    #[test]
    fn test_validate_string_literal() {
        assert!(validate("name = \"Alice\"").is_ok());
    }

    #[test]
    fn test_validate_error_mismatched_parens() {
        assert!(validate("(a + b").is_err());
    }

    #[test]
    fn test_validate_error_trailing_operator() {
        assert!(validate("a +").is_err());
    }
}