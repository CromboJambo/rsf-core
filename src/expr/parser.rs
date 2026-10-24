//! Expression parser for RSF table filtering.
//!
//! Recursive descent parser with explicit precedence levels:
//! - Lowest: OR (left-associative)
//! - Higher: AND (left-associative)
//! - Highest: NOT (right-associative prefix)
//! - Comparison/Equality operators
//! - Additive operators (+, -)
//! - Multiplicative operators (*, /)
//! - Unary minus (prefix -)
//! - Primary expressions (literals, identifiers, parenthesized sub-expressions)

use crate::expr::lexer::tokenize;
use crate::expr::types::{BinLogOp, BinOp, Expr, ParseError, Token};

/// Parse an expression string into an AST.
pub fn parse(input: &str, column_names: &[String]) -> Result<Expr, ParseError> {
    let tokens = tokenize(input);
    let mut parser = Parser {
        tokens,
        pos: 0,
        column_names: column_names.to_vec(),
    };
    let expr = parser.parse_expr()?;

    if parser.pos < parser.tokens.len() {
        return Err(ParseError(format!(
            "Unexpected token after expression: {}",
            parser.tokens[parser.pos]
        )));
    }

    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    column_names: Vec<String>,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Parse a complete expression (OR level - lowest precedence).
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;

        while matches!(self.peek(), Some(Token::Or)) {
            self.advance(); // consume OR
            let right = self.parse_and()?;
            left = Expr::LogicalOp(BinLogOp::Or, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// Parse AND expressions.
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;

        while matches!(self.peek(), Some(Token::And)) {
            self.advance(); // consume AND
            let right = self.parse_not()?;
            left = Expr::LogicalOp(BinLogOp::And, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    /// Parse NOT expressions (unary prefix).
    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.advance(); // consume NOT
            let expr = self.parse_not()?;
            return Ok(Expr::Not(Box::new(expr)));
        }

        self.parse_comparison()
    }

    /// Parse comparison and equality expressions.
    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_additive()?;

        match self.peek() {
            Some(Token::Eq) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Eq, Box::new(right)))
            }
            Some(Token::Neq) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Neq, Box::new(right)))
            }
            Some(Token::Gt) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Gt, Box::new(right)))
            }
            Some(Token::Lt) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Lt, Box::new(right)))
            }
            Some(Token::Ge) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Ge, Box::new(right)))
            }
            Some(Token::Le) => {
                self.advance();
                let right = self.parse_additive()?;
                Ok(Expr::BinaryOp(Box::new(left), BinOp::Le, Box::new(right)))
            }
            _ => Ok(left),
        }
    }

    /// Parse additive expressions (+ and -).
    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_multiplicative()?;

        while matches!(self.peek(), Some(Token::Plus) | Some(Token::Minus)) {
            let op = match self.advance() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_multiplicative()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }

        Ok(left)
    }

    /// Parse multiplicative expressions (* and /).
    fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;

        while matches!(self.peek(), Some(Token::Star) | Some(Token::Slash)) {
            let op = match self.advance() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            left = Expr::BinaryOp(Box::new(left), op, Box::new(right));
        }

        Ok(left)
    }

    /// Parse unary expressions (prefix -).
    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), Some(Token::Minus)) {
            self.advance(); // consume minus
            let expr = self.parse_unary()?;
            return Ok(Expr::Negate(Box::new(expr)));
        }

        self.parse_primary()
    }

    /// Parse primary expressions (literals, identifiers, parenthesized).
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(Token::Ident(name)) => {
                self.advance();
                // Validate column name - unknown identifiers become literals
                if !self.column_names.contains(&name) {
                    return Ok(Expr::Literal(crate::table::FieldValue::Text(name)));
                }
                // Check for IN clause
                if matches!(self.peek(), Some(Token::In)) {
                    return self.parse_in_clause(name);
                }
                // Check for IS NULL / IS NOT NULL
                if matches!(self.peek(), Some(Token::Is)) {
                    return self.parse_is_null(name);
                }
                Ok(Expr::Column(name))
            }
            Some(Token::StringLit(s)) => {
                self.advance();
                Ok(Expr::Literal(crate::table::FieldValue::Text(s)))
            }
            Some(Token::Number(n)) => {
                self.advance();
                // Try to preserve integer type if possible
                if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                    Ok(Expr::Literal(crate::table::FieldValue::Integer(n as i64)))
                } else {
                    Ok(Expr::Literal(crate::table::FieldValue::Float(n)))
                }
            }
            Some(Token::Bool(b)) => {
                self.advance();
                Ok(Expr::Literal(crate::table::FieldValue::Boolean(b)))
            }
            Some(Token::Null) => {
                self.advance();
                Ok(Expr::Literal(crate::table::FieldValue::Null))
            }
            Some(Token::LParen) => {
                self.advance(); // consume (
                let expr = self.parse_expr()?;
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return Err(ParseError("Missing closing parenthesis".into()));
                }
                self.advance(); // consume )
                Ok(expr)
            }
            _ => Err(ParseError(format!(
                "Unexpected token: {:?}",
                self.peek()
            ))),
        }
    }

    /// Parse IN (val1, val2, ...) clause.
    fn parse_in_clause(&mut self, col_name: String) -> Result<Expr, ParseError> {
        self.advance(); // consume IN

        if !matches!(self.peek(), Some(Token::LParen)) {
            return Err(ParseError("IN requires parenthesized list".into()));
        }
        self.advance(); // consume (

        let mut values = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(Token::StringLit(s)) => {
                    self.advance();
                    values.push(crate::table::FieldValue::Text(s));
                }
                Some(Token::Number(n)) => {
                    self.advance();
                    if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                        values.push(crate::table::FieldValue::Integer(n as i64));
                    } else {
                        values.push(crate::table::FieldValue::Float(n));
                    }
                }
                Some(Token::Bool(b)) => {
                    self.advance();
                    values.push(crate::table::FieldValue::Boolean(b));
                }
                Some(Token::Null) => {
                    self.advance();
                    values.push(crate::table::FieldValue::Null);
                }
                _ => return Err(ParseError("Unexpected token in IN list".into())),
            }

            match self.peek() {
                Some(Token::Comma) => {
                    self.advance(); // consume comma
                }
                Some(Token::RParen) => {
                    self.advance(); // consume )
                    break;
                }
                _ => return Err(ParseError("Expected , or ) in IN list".into())),
            }
        }

        Ok(Expr::In(col_name, values))
    }

    /// Parse IS NULL / IS NOT NULL clause.
    fn parse_is_null(&mut self, col_name: String) -> Result<Expr, ParseError> {
        self.advance(); // consume IS

        let is_not = matches!(self.peek(), Some(Token::Not));
        if is_not {
            self.advance(); // consume NOT
        }

        if !matches!(self.peek(), Some(Token::Null)) {
            return Err(ParseError("IS requires NULL".into()));
        }
        self.advance(); // consume NULL

        Ok(Expr::NullCheck(col_name, is_not))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_comparison() {
        let expr = parse("Amount > 100", &["Amount".to_string()]).unwrap();
        match expr {
            Expr::BinaryOp(left, BinOp::Gt, right) => {
                assert!(matches!(*left, Expr::Column(ref n) if n == "Amount"));
                assert!(matches!(*right, Expr::Literal(crate::table::FieldValue::Integer(100))));
            }
            _ => panic!("Expected BinaryOp(Gt), got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_string_comparison() {
        let expr = parse("name = \"Alice\"", &["name".to_string()]).unwrap();
        match expr {
            Expr::BinaryOp(left, BinOp::Eq, right) => {
                assert!(matches!(*left, Expr::Column(ref n) if n == "name"));
                assert!(matches!(*right, Expr::Literal(crate::table::FieldValue::Text(ref s)) if s == "Alice"));
            }
            _ => panic!("Expected BinaryOp(Eq), got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_and_or_precedence() {
        // a AND b OR c should parse as (a AND b) OR c
        let expr = parse("a AND b OR c", &["a".to_string(), "c".to_string(), "b".to_string()]).unwrap();
        match expr {
            Expr::LogicalOp(BinLogOp::Or, left, right) => {
                assert!(matches!(*left, Expr::LogicalOp(BinLogOp::And, _, _)));
                assert!(matches!(*right, Expr::Column(ref n) if n == "c"));
            }
            _ => panic!("Expected Or at top level, got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_parentheses() {
        let expr = parse("a AND (b OR c)", &["a".to_string(), "c".to_string(), "b".to_string()]).unwrap();
        // Top level should be AND with right side being OR
        match expr {
            Expr::LogicalOp(BinLogOp::And, _, right) => {
                assert!(matches!(*right, Expr::LogicalOp(BinLogOp::Or, _, _)));
            }
            _ => panic!("Expected And at top level, got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_not() {
        let expr = parse("NOT a", &["a".to_string()]).unwrap();
        assert!(matches!(expr, Expr::Not(_)));
    }

    #[test]
    fn test_parse_in_clause() {
        let expr = parse("id IN (1, 2, 3)", &["id".to_string()]).unwrap();
        match expr {
            Expr::In(col, vals) => {
                assert_eq!(col, "id");
                assert_eq!(vals.len(), 3);
            }
            _ => panic!("Expected In expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let expr = parse("x IS NULL", &["x".to_string()]).unwrap();
        match expr {
            Expr::NullCheck(col, is_not) => {
                assert_eq!(col, "x");
                assert!(!is_not);
            }
            _ => panic!("Expected NullCheck, got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_is_not_null() {
        let expr = parse("x IS NOT NULL", &["x".to_string()]).unwrap();
        match expr {
            Expr::NullCheck(col, is_not) => {
                assert_eq!(col, "x");
                assert!(is_not);
            }
            _ => panic!("Expected NullCheck(is_not), got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_arithmetic() {
        let expr = parse("a + b * c", &["b".to_string(), "a".to_string(), "c".to_string()]).unwrap();
        // Should be a + (b * c) due to precedence
        match expr {
            Expr::BinaryOp(_, BinOp::Add, right) => {
                assert!(matches!(*right, Expr::BinaryOp(_, BinOp::Mul, _)));
            }
            _ => panic!("Expected Add at top level, got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_negative_number() {
        let expr = parse("a > -5", &["a".to_string()]).unwrap();
        match expr {
            Expr::BinaryOp(_, BinOp::Gt, right) => {
                assert!(matches!(*right, Expr::Literal(crate::table::FieldValue::Integer(-5))));
            }
            _ => panic!("Expected BinaryOp(Gt), got {:?}", expr),
        }
    }

    #[test]
    fn test_parse_complex_expression() {
        let expr = parse("amount > 100 AND (status = 'active' OR status = 'pending')", &["amount".to_string(), "active".to_string(), "status".to_string(), "pending".to_string()]).unwrap();
        // Top level should be AND
        assert!(matches!(expr, Expr::LogicalOp(BinLogOp::And, _, _)));
    }

    #[test]
    fn test_parse_error_unexpected_token() {
        let result = parse("a +", &["a".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_mismatched_parens() {
        let result = parse("(a + b", &["b".to_string(), "a".to_string()]);
        assert!(result.is_err());
    }
}
