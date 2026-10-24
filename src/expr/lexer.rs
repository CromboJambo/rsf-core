//! Expression tokenizer for RSF table filtering.
//!
//! Converts expression strings into a stream of tokens for the parser.
//! Handles identifiers, string/number/bool/null literals, operators, and keywords.
//! Context-sensitive disambiguation: `-` is unary minus after `(` or another operator,
//! binary minus otherwise; folded negative numbers (`-5`) are produced when `-`
//! follows a value-ending token.

use crate::expr::types::{BinOp, Token};

/// Tokens that end a value (so the next `-` must be binary Sub).
fn is_value_ending(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Ident(_)
            | Token::StringLit(_)
            | Token::Number(_)
            | Token::Bool(_)
            | Token::Null
            | Token::RParen
    )
}

/// Tokens that end an expression (so the next `-` must be binary Sub).
fn is_expr_ending(tok: &Token) -> bool {
    matches!(tok, Token::And | Token::Or)
}

/// Tokens after which a leading `-` becomes unary Negate.
fn is_unary_context(tok: &Token) -> bool {
    matches!(
        tok,
        Token::LParen
            | Token::Eq
            | Token::Neq
            | Token::Gt
            | Token::Lt
            | Token::Ge
            | Token::Le
            | Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::Comma
    )
}

/// Tokens that are operators (not values).
fn is_operator(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Eq
            | Token::Neq
            | Token::Gt
            | Token::Lt
            | Token::Ge
            | Token::Le
            | Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
    )
}

/// Tokens that are keywords.
fn is_keyword(tok: &Token) -> bool {
    matches!(tok, Token::And | Token::Or | Token::Not | Token::Is | Token::In)
}

/// Tokenize an expression string into a sequence of tokens.
pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        // String literal
        if chars[i] == '"' || chars[i] == '\'' {
            let quote = chars[i];
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    match chars[i + 1] {
                        'n' => {
                            s.push('\n');
                            i += 2;
                        }
                        't' => {
                            s.push('\t');
                            i += 2;
                        }
                        '\\' | '"' | '\'' => {
                            s.push(chars[i + 1]);
                            i += 2;
                        }
                        _ => {
                            s.push(chars[i]);
                            i += 1;
                        }
                    }
                } else {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            if i < chars.len() && chars[i] == quote {
                i += 1; // consume closing quote
            }
            tokens.push(Token::StringLit(s));
        }
        // Number literal
        else if chars[i].is_ascii_digit() || (chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let mut num = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                num.push(chars[i]);
                i += 1;
            }
            if let Ok(n) = num.parse::<f64>() {
                tokens.push(Token::Number(n));
            } else {
                panic!("Invalid number: {}", num);
            }
        }
        // Identifier or keyword
        else if chars[i].is_alphabetic() || chars[i] == '_' {
            let mut ident = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                ident.push(chars[i]);
                i += 1;
            }
            // Check for keywords
            match ident.to_uppercase().as_str() {
                "AND" => tokens.push(Token::And),
                "OR" => tokens.push(Token::Or),
                "NOT" => tokens.push(Token::Not),
                "IS" => tokens.push(Token::Is),
                "IN" => tokens.push(Token::In),
                "TRUE" | "YES" | "1" => tokens.push(Token::Bool(true)),
                "FALSE" | "NO" | "0" => tokens.push(Token::Bool(false)),
                "NULL" => tokens.push(Token::Null),
                _ => tokens.push(Token::Ident(ident)),
            }
        }
        // Two-character operators
        else if i + 1 < chars.len() {
            let two = format!("{}{}", chars[i], chars[i + 1]);
            match two.as_str() {
                ">=" => {
                    tokens.push(Token::Ge);
                    i += 2;
                }
                "<=" => {
                    tokens.push(Token::Le);
                    i += 2;
                }
                "!=" => {
                    tokens.push(Token::Neq);
                    i += 2;
                }
                _ => {
                    // Single character operators
                    match chars[i] {
                        '=' => {
                            tokens.push(Token::Eq);
                            i += 1;
                        }
                        '>' => {
                            tokens.push(Token::Gt);
                            i += 1;
                        }
                        '<' => {
                            tokens.push(Token::Lt);
                            i += 1;
                        }
                        '+' => {
                            tokens.push(Token::Plus);
                            i += 1;
                        }
                        '-' => {
                            // Context-sensitive: unary or binary minus
                            if tokens.is_empty() || is_unary_context(tokens.last().unwrap()) {
                                // Peek ahead to see if this is a negative number literal
                                let mut j = i + 1;
                                while j < chars.len() && chars[j].is_whitespace() {
                                    j += 1;
                                }
                                if j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                                    // Fold into negative number literal
                                    let mut num = String::new();
                                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                                        num.push(chars[j]);
                                        j += 1;
                                    }
                                    if let Ok(n) = num.parse::<f64>() {
                                        tokens.push(Token::Number(-n));
                                    }
                                    i = j;
                                } else {
                                    // Unary minus operator (will be handled by parser)
                                    tokens.push(Token::Minus);
                                    i += 1;
                                }
                            } else if is_value_ending(tokens.last().unwrap()) || is_expr_ending(tokens.last().unwrap()) {
                                // Binary minus
                                tokens.push(Token::Minus);
                                i += 1;
                            } else {
                                // Default: binary minus
                                tokens.push(Token::Minus);
                                i += 1;
                            }
                        }
                        '*' => {
                            tokens.push(Token::Star);
                            i += 1;
                        }
                        '/' => {
                            tokens.push(Token::Slash);
                            i += 1;
                        }
                        '(' => {
                            tokens.push(Token::LParen);
                            i += 1;
                        }
                        ')' => {
                            tokens.push(Token::RParen);
                            i += 1;
                        }
                        ',' => {
                            tokens.push(Token::Comma);
                            i += 1;
                        }
                        _ => {
                            panic!("Unexpected character: {}", chars[i]);
                        }
                    }
                }
            }
        } else {
            // Single character at end
            match chars[i] {
                '=' => {
                    tokens.push(Token::Eq);
                    i += 1;
                }
                '>' => {
                    tokens.push(Token::Gt);
                    i += 1;
                }
                '<' => {
                    tokens.push(Token::Lt);
                    i += 1;
                }
                '+' => {
                    tokens.push(Token::Plus);
                    i += 1;
                }
                '-' => {
                    // Unary minus at end (incomplete expression)
                    tokens.push(Token::Minus);
                    i += 1;
                }
                '*' => {
                    tokens.push(Token::Star);
                    i += 1;
                }
                '/' => {
                    tokens.push(Token::Slash);
                    i += 1;
                }
                '(' => {
                    tokens.push(Token::LParen);
                    i += 1;
                }
                ')' => {
                    tokens.push(Token::RParen);
                    i += 1;
                }
                ',' => {
                    tokens.push(Token::Comma);
                    i += 1;
                }
                _ => panic!("Unexpected character: {}", chars[i]),
            }
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple_comparison() {
        let tokens = tokenize("Amount > 100");
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[0], Token::Ident(s) if s == "Amount"));
        assert!(matches!(&tokens[1], Token::Gt));
        assert!(matches!(&tokens[2], Token::Number(100.0)));
    }

    #[test]
    fn test_tokenize_string_literal() {
        let tokens = tokenize("name = \"Alice\"");
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[2], Token::StringLit(s) if s == "Alice"));
    }

    #[test]
    fn test_tokenize_negative_number() {
        let tokens = tokenize("a > -5");
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[2], Token::Number(n) if *n == -5.0));
    }

    #[test]
    fn test_tokenize_unary_minus() {
        let tokens = tokenize("a + -b");
        // Should have: a, Plus, Minus (unary), b
        assert_eq!(tokens.len(), 4);
        assert!(matches!(&tokens[2], Token::Minus));
    }

    #[test]
    fn test_tokenize_keywords() {
        let tokens = tokenize("a AND b OR c");
        assert_eq!(tokens.len(), 5);
        assert!(matches!(&tokens[1], Token::And));
        assert!(matches!(&tokens[3], Token::Or));
    }

    #[test]
    fn test_tokenize_in_clause() {
        let tokens = tokenize("id IN (1, 2, 3)");
        assert_eq!(tokens.len(), 9);
        assert!(matches!(&tokens[1], Token::In));
        assert!(matches!(&tokens[2], Token::LParen));
    }

    #[test]
    fn test_tokenize_null() {
        let tokens = tokenize("x IS NULL");
        assert_eq!(tokens.len(), 3);
        assert!(matches!(&tokens[1], Token::Is));
        assert!(matches!(&tokens[2], Token::Null));
    }

    #[test]
    fn test_tokenize_is_not_null() {
        let tokens = tokenize("x IS NOT NULL");
        assert_eq!(tokens.len(), 4);
        assert!(matches!(&tokens[1], Token::Is));
        assert!(matches!(&tokens[2], Token::Not));
        assert!(matches!(&tokens[3], Token::Null));
    }

    #[test]
    fn test_tokenize_parentheses() {
        let tokens = tokenize("(a + b) * c");
        assert_eq!(tokens.len(), 7);
        assert!(matches!(&tokens[0], Token::LParen));
        assert!(matches!(&tokens[4], Token::RParen));
    }

    #[test]
    fn test_tokenize_arithmetic_ops() {
        let tokens = tokenize("a + b * c - d / e");
        assert_eq!(tokens.len(), 9);
        assert!(matches!(&tokens[1], Token::Plus));
        assert!(matches!(&tokens[3], Token::Star));
        assert!(matches!(&tokens[5], Token::Minus));
        assert!(matches!(&tokens[7], Token::Slash));
    }

    #[test]
    fn test_tokenize_all_comparison_ops() {
        let tokens = tokenize("a = b");
        assert!(matches!(&tokens[1], Token::Eq));

        let tokens = tokenize("a != b");
        assert!(matches!(&tokens[1], Token::Neq));

        let tokens = tokenize("a >= b");
        assert!(matches!(&tokens[1], Token::Ge));

        let tokens = tokenize("a <= b");
        assert!(matches!(&tokens[1], Token::Le));
    }
}
