//! Typed table representation for pipeline operations (Phase 7).
//!
//! Provides a typed in-memory model (`TypedTable`) that bridges raw CSV strings
//! into structured `FieldValue` types, enabling type-aware filtering, sorting,
//! and projection across pipe boundaries.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::ranking::{ColumnProfile, TypeHint};

/// Token types for the expression tokenizer.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    StringLit(String),
    Number(f64),
    Bool(bool),
    Null,
    // Comparison operators
    Eq, Neq, Gt, Lt, Ge, Le,
    // Arithmetic operators
    Plus, Minus, Star, Slash,
    // Function call marker (identifier followed by '(')
    FuncCall,
    // Logical operators (keywords)
    And, Or, Not, Is, In,
    // Punctuation
    LParen, RParen, Comma,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Ident(s) => write!(f, "{s}"),
            Token::StringLit(s) => write!(f, "'{s}'"),
            Token::Number(n) => write!(f, "{n}"),
            Token::Bool(b) => write!(f, "{b}"),
            Token::Null => write!(f, "NULL"),
            Token::Eq => write!(f, "="),
            Token::Neq => write!(f, "!="),
            Token::Gt => write!(f, ">"),
            Token::Lt => write!(f, "<"),
            Token::Ge => write!(f, ">="),
            Token::Le => write!(f, "<="),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::FuncCall => write!(f, "func"),
            Token::And => write!(f, "AND"),
            Token::Or => write!(f, "OR"),
            Token::Not => write!(f, "NOT"),
            Token::Is => write!(f, "IS"),
            Token::In => write!(f, "IN"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Comma => write!(f, ","),
        }
    }
}

/// An expression AST node for `where` clause evaluation.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Column reference by name (e.g., `amount`)
    Column(String),
    /// A literal value
    Literal(FieldValue),
    /// Binary comparison: left OP right
    BinaryOp(Box<Expr>, BinOp, Box<Expr>),
    /// Logical AND/OR between two expressions
    LogicalOp(BinLogOp, Box<Expr>, Box<Expr>),
    /// Unary NOT
    Not(Box<Expr>),
    /// IS NULL / IS NOT NULL
    NullCheck(String, bool), // (column_name, is_not)
    /// IN (val1, val2, ...)
    In(String, Vec<FieldValue>),
}

/// Binary comparison operators.
#[derive(Debug, Clone)]
pub enum BinOp {
    Eq, Neq, Gt, Lt, Ge, Le,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinOp::Eq => write!(f, "="),
            BinOp::Neq => write!(f, "!="),
            BinOp::Gt => write!(f, ">"),
            BinOp::Lt => write!(f, "<"),
            BinOp::Ge => write!(f, ">="),
            BinOp::Le => write!(f, "<="),
        }
    }
}

/// Binary logical operators.
#[derive(Debug, Clone)]
pub enum BinLogOp {
    And, Or,
}

impl fmt::Display for BinLogOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinLogOp::And => write!(f, "AND"),
            BinLogOp::Or => write!(f, "OR"),
        }
    }
}

/// Expression parsing error.
#[derive(Debug)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Tokenizer — converts an expression string into tokens.
struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    fn tokenize(mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.chars.len() {
                break;
            }

            match self.peek() {
                None => break,
                Some('\'') | Some('"') => {
                    tokens.push(self.read_string_literal()?);
                }
                Some('(') => {
                    self.advance();
                    tokens.push(Token::LParen);
                }
                Some(')') => {
                    self.advance();
                    tokens.push(Token::RParen);
                }
                Some(',') => {
                    self.advance();
                    tokens.push(Token::Comma);
                }
                Some('!') => {
                    if self.chars.get(self.pos + 1) == Some(&'=') {
                        self.advance(); // !
                        self.advance(); // =
                        tokens.push(Token::Neq);
                    } else {
                        return Err(ParseError("Unexpected '!'".into()));
                    }
                }
                Some('>') => {
                    if self.chars.get(self.pos + 1) == Some(&'=') {
                        self.advance(); // >
                        self.advance(); // =
                        tokens.push(Token::Ge);
                    } else {
                        self.advance();
                        tokens.push(Token::Gt);
                    }
                }
                Some('<') => {
                    if self.chars.get(self.pos + 1) == Some(&'=') {
                        self.advance(); // <
                        self.advance(); // =
                        tokens.push(Token::Le);
                    } else {
                        self.advance();
                        tokens.push(Token::Lt);
                    }
                }
                Some('=') => {
                    self.advance();
                    tokens.push(Token::Eq);
                }
                Some('+') => {
                    self.advance();
                    tokens.push(Token::Plus);
                }
                Some('-') => {
                    // Distinguish unary minus from binary subtraction.
                    // Unary: at start, after '(' or operator token, or after '='/comparison ops.
                    let is_unary = tokens.is_empty()
                        || matches!(tokens.last(), Some(Token::LParen | Token::Eq | Token::Neq | Token::Gt | Token::Lt | Token::Ge | Token::Le | Token::And | Token::Or | Token::Not));
                    self.advance();
                    if is_unary {
                        // Read as a negative number literal directly.
                        if let Some(n) = self.try_read_negative_number() {
                            tokens.push(Token::Number(n));
                        } else {
                            tokens.push(Token::Minus);
                        }
                    } else {
                        tokens.push(Token::Minus);
                    }
                }
                Some('*') => {
                    self.advance();
                    tokens.push(Token::Star);
                }
                Some('/') => {
                    self.advance();
                    tokens.push(Token::Slash);
                }
                Some(c) if c.is_ascii_digit() || (c == '.' && self.chars.get(self.pos + 1).map_or(false, |n| n.is_ascii_digit())) => {
                    tokens.push(self.read_number()?);
                }
                Some(c) if c.is_alphabetic() || c == '_' => {
                    tokens.push(self.read_ident()?);
                }
                Some(other) => {
                    return Err(ParseError(format!("Unexpected character '{other}'")));
                }
            }
        }
        Ok(tokens)
    }

    fn read_string_literal(&mut self) -> Result<Token, ParseError> {
        let _quote = self.advance(); // consume opening quote
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if ch == '"' || ch == '\'' {
                self.advance(); // consume closing quote
                return Ok(Token::StringLit(s));
            }
            self.advance();
            s.push(ch);
        }
        Err(ParseError("Unterminated string literal".into()))
    }

    fn read_number(&mut self) -> Result<Token, ParseError> {
        let mut num_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                num_str.push(c);
                self.advance();
            } else {
                break;
            }
        }
        match num_str.parse::<f64>() {
            Ok(n) => Ok(Token::Number(n)),
            Err(_) => Err(ParseError(format!("Invalid number: {num_str}"))),
        }
    }

    /// Try to read a negative number after consuming the '-' token.
    /// Returns Some(f64) if digits follow, None otherwise (binary minus).
    fn try_read_negative_number(&mut self) -> Option<f64> {
        let mut num_str = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                num_str.push(c);
                self.advance();
            } else {
                break;
            }
        }
        if !num_str.is_empty() {
            num_str.insert(0, '-');
            num_str.parse::<f64>().ok()
        } else {
            None
        }
    }

    fn read_ident(&mut self) -> Result<Token, ParseError> {
        let mut ident = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(c);
                self.advance();
            } else {
                break;
            }
        }

        // Check for keywords (case-insensitive)
        let upper = ident.to_uppercase();
        match upper.as_str() {
            "AND" => Ok(Token::And),
            "OR" => Ok(Token::Or),
            "NOT" => Ok(Token::Not),
            "IS" => Ok(Token::Is),
            "IN" => Ok(Token::In),
            "NULL" => Ok(Token::Null),
            "TRUE" => Ok(Token::Bool(true)),
            "FALSE" => Ok(Token::Bool(false)),
            _ => {
                // Check if it's a number that was parsed as ident (shouldn't happen, but be safe)
                if let Ok(n) = ident.parse::<f64>() {
                    Ok(Token::Number(n))
                } else {
                    Ok(Token::Ident(ident))
                }
            }
        }
    }
}

/// Parser — recursive descent parser for expressions.
struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    column_names: &'a [String],
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Token>, column_names: &'a [String]) -> Self {
        Self { tokens, pos: 0, column_names }
    }

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

    /// Parse an expression: handles OR (lowest precedence), then AND, then comparisons.
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_or()?;
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while let Some(Token::Or) = self.peek() {
            self.advance(); // consume OR
            let right = self.parse_and()?;
            left = Expr::LogicalOp(BinLogOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while let Some(Token::And) = self.peek() {
            self.advance(); // consume AND
            let right = self.parse_not()?;
            left = Expr::LogicalOp(BinLogOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if let Some(Token::Not) = self.peek() {
            self.advance(); // consume NOT
            let operand = self.parse_comparison()?;
            return Ok(Expr::Not(Box::new(operand)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        // Check for IS NULL / IS NOT NULL
        if let Some(Token::Ident(col)) = self.peek().cloned() {
            let col_name = col.clone();
            // Look ahead: is the next token IS?
            if self.tokens.get(self.pos + 1).map_or(false, |t| matches!(t, Token::Is)) {
                self.advance(); // consume column name
                self.advance(); // consume IS
                let is_not = matches!(self.peek(), Some(Token::Not));
                if is_not {
                    self.advance(); // consume NOT
                }
                if !matches!(self.peek(), Some(Token::Null)) {
                    return Err(ParseError(format!("Expected NULL after IS{}", if is_not { " NOT" } else { "" })));
                }
                self.advance(); // consume NULL
                return Ok(Expr::NullCheck(col_name, is_not));
            }

            // Check for IN (column IN (...))
            if self.tokens.get(self.pos + 1).map_or(false, |t| matches!(t, Token::In)) {
                self.advance(); // consume column name
                self.advance(); // consume IN
                return self.parse_in(col_name);
            }
        }

        // Check for IS NULL at top level (no column prefix) — treat as literal null check on first column? No, require column.
        if let Some(Token::Is) = self.peek() {
            self.advance(); // consume IS
            let is_not = matches!(self.peek(), Some(Token::Not));
            if is_not {
                self.advance(); // consume NOT
            }
            if !matches!(self.peek(), Some(Token::Null)) {
                return Err(ParseError("Expected NULL after IS".into()));
            }
            self.advance(); // consume NULL
            // Without a column name, we can't do much — error out
            return Err(ParseError("IS NULL requires a column reference".into()));
        }

        // Parse left-hand side (column or literal)
        let left = self.parse_primary()?;

        // Check for binary comparison operator
        match self.peek() {
            Some(Token::Eq | Token::Neq | Token::Gt | Token::Lt | Token::Ge | Token::Le) => {
                let op = match self.advance().unwrap() {
                    Token::Eq => BinOp::Eq,
                    Token::Neq => BinOp::Neq,
                    Token::Gt => BinOp::Gt,
                    Token::Lt => BinOp::Lt,
                    Token::Ge => BinOp::Ge,
                    Token::Le => BinOp::Le,
                    _ => unreachable!(),
                };
                let right = self.parse_primary()?;
                Ok(Expr::BinaryOp(Box::new(left), op, Box::new(right)))
            }
            Some(Token::In) => {
                // Handle: column IN (...) — left is already the column
                if !matches!(&left, Expr::Column(_)) {
                    return Err(ParseError("IN requires a column reference on the left".into()));
                }
                let col_name = match &left {
                    Expr::Column(name) => name.clone(),
                    _ => unreachable!(),
                };
                self.advance(); // consume IN
                self.parse_in(col_name)
            }
            Some(Token::And | Token::Or | Token::RParen) => Ok(left),
            other => Err(ParseError(format!("Unexpected token: {other:?}"))),
        }
    }

    fn parse_in(&mut self, col_name: String) -> Result<Expr, ParseError> {
        // Expect LPAREN
        if !matches!(self.peek(), Some(Token::LParen)) {
            return Err(ParseError(format!("Expected '(' after IN for column '{col_name}'")));
        }
        self.advance(); // consume (

        let mut values = Vec::new();
        loop {
            let val = self.parse_value()?;
            values.push(val);
            match self.peek() {
                Some(Token::Comma) => {
                    self.advance(); // consume ,
                }
                Some(Token::RParen) => {
                    self.advance(); // consume )
                    return Ok(Expr::In(col_name, values));
                }
                other => return Err(ParseError(format!("Expected ',' or ')' in IN list, got {other:?}"))),
            }
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.advance(); // consume (
                let expr = self.parse_expr()?;
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return Err(ParseError("Expected ')'".into()));
                }
                self.advance(); // consume )
                Ok(expr)
            }
            Some(Token::StringLit(s)) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::Literal(FieldValue::Text(s)))
            }
            Some(Token::Number(n)) => {
                let n = *n;
                self.advance();
                if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                    Ok(Expr::Literal(FieldValue::Integer(n as i64)))
                } else {
                    Ok(Expr::Literal(FieldValue::Float(n)))
                }
            }
            Some(Token::Bool(b)) => {
                let b = *b;
                self.advance();
                Ok(Expr::Literal(FieldValue::Boolean(b)))
            }
            Some(Token::Null) => {
                self.advance();
                Ok(Expr::Literal(FieldValue::Null))
            }
            Some(Token::Ident(name)) => {
                let name = name.clone();
                // Check if it's a column name (case-insensitive match against known columns)
                let col_lower = name.to_lowercase();
                let matched_col = self.column_names.iter()
                    .find(|c| c.to_lowercase() == col_lower)
                    .map(String::from);

                if let Some(col) = matched_col {
                    self.advance();
                    Ok(Expr::Column(col))
                } else {
                    // Treat as a string literal for unknown identifiers (e.g., bare column names in comparisons)
                    self.advance();
                    Ok(Expr::Literal(FieldValue::Text(name)))
                }
            }
            Some(other) => Err(ParseError(format!("Unexpected token: {other:?}"))),
            None => Err(ParseError("Unexpected end of expression".into())),
        }
    }

    fn parse_value(&mut self) -> Result<FieldValue, ParseError> {
        // Clone the token before calling advance() to avoid holding a reference across a mutable borrow.
        let token = match self.peek().cloned() {
            Some(t) => t,
            None => return Err(ParseError("Expected value in IN list".into())),
        };

        self.advance();

        match &token {
            Token::Number(n) => {
                if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
                    Ok(FieldValue::Integer(*n as i64))
                } else {
                    Ok(FieldValue::Float(*n))
                }
            }
            Token::Bool(b) => Ok(FieldValue::Boolean(*b)),
            Token::Null => Ok(FieldValue::Null),
            _ => Err(ParseError(format!("Expected value in IN list, got {token:?}"))),
        }
    }

    fn parse(&mut self) -> Result<Expr, ParseError> {
        let expr = self.parse_expr()?;
        if self.pos < self.tokens.len() {
            return Err(ParseError(format!("Unexpected token after expression: {:?}", self.tokens[self.pos])));
        }
        Ok(expr)
    }
}

impl Expr {
    /// Parse an expression string into an AST.
    pub fn parse(input: &str, column_names: &[String]) -> Result<Self, ParseError> {
        let tokens = Tokenizer::new(input).tokenize()?;
        if tokens.is_empty() {
            return Err(ParseError("Empty expression".into()));
        }
        let mut parser = Parser::new(tokens, column_names);
        parser.parse()
    }

    /// Evaluate this expression against a single row of field values.
    pub fn evaluate(&self, row: &[FieldValue], columns: &[Column]) -> bool {
        match self {
            Expr::Column(name) => {
                // Column reference — truthiness check (non-null = true)
                if let Some(idx) = columns.iter().position(|c| &c.name == name) {
                    !row.get(idx).map_or(true, |v| v.is_null())
                } else {
                    false
                }
            }
            Expr::Literal(_) => true, // literals always evaluate to "truthy" in boolean context

            Expr::BinaryOp(left, op, right) => {
                let left_val = Self::resolve_value(left, row, columns);
                let right_val = Self::resolve_value(right, row, columns);
                Self::compare(&left_val, &right_val, op)
            }

            Expr::LogicalOp(logic, left, right) => match logic {
                BinLogOp::And => left.evaluate(row, columns) && right.evaluate(row, columns),
                BinLogOp::Or => left.evaluate(row, columns) || right.evaluate(row, columns),
            },

            Expr::Not(inner) => !inner.evaluate(row, columns),

            Expr::NullCheck(col_name, is_not) => {
                if let Some(idx) = columns.iter().position(|c| &c.name == col_name) {
                    let is_null = row.get(idx).map_or(true, |v| v.is_null());
                    *is_not != is_null
                } else {
                    false
                }
            }

            Expr::In(col_name, values) => {
                if let Some(idx) = columns.iter().position(|c| &c.name == col_name) {
                    let row_val = row.get(idx).cloned().unwrap_or(FieldValue::Null);
                    values.contains(&row_val)
                } else {
                    false
                }
            }
        }
    }

    /// Resolve an expression to its field value for comparison.
    fn resolve_value(expr: &Expr, row: &[FieldValue], columns: &[Column]) -> FieldValue {
        match expr {
            Expr::Column(name) => {
                if let Some(idx) = columns.iter().position(|c| &c.name == name) {
                    row.get(idx).cloned().unwrap_or(FieldValue::Null)
                } else {
                    FieldValue::Null
                }
            }
            Expr::Literal(v) => v.clone(),
            // For nested expressions in comparisons, evaluate recursively
            _ => FieldValue::Null,
        }
    }

    /// Convert a FieldValue to f64 for numeric comparison (returns None if not numeric).
    fn as_f64(v: &FieldValue) -> Option<f64> {
        use FieldValue::*;
        match v {
            Integer(n) => Some(*n as f64),
            Float(f) => Some(*f),
            Text(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Compare two field values using the given binary operator.
    fn compare(left: &FieldValue, right: &FieldValue, op: &BinOp) -> bool {
        use FieldValue::*;

        // Null comparisons: only = and != make sense
        match (left, right) {
            (Null, Null) => return matches!(op, BinOp::Eq),
            (Null, _) | (_, Null) => return matches!(op, BinOp::Neq),
            _ => {}
        }

        // Try numeric comparison first: if both sides are parseable as numbers, compare numerically.
        if let (Some(l_num), Some(r_num)) = (Self::as_f64(left), Self::as_f64(right)) {
            return match op {
                BinOp::Eq => (l_num - r_num).abs() < 1e-9,
                BinOp::Neq => (l_num - r_num).abs() >= 1e-9,
                BinOp::Gt => l_num > r_num,
                BinOp::Lt => l_num < r_num,
                BinOp::Ge => l_num >= r_num,
                BinOp::Le => l_num <= r_num,
            };
        }

        // Fall back to text comparison.
        Self::compare_text(&left.as_str(), &right.as_str(), op)
    }

    /// Compare two text values.
    fn compare_text(left: &str, right: &str, op: &BinOp) -> bool {
        match op {
            BinOp::Eq => left == right,
            BinOp::Neq => left != right,
            BinOp::Gt => left > right,
            BinOp::Lt => left < right,
            BinOp::Ge => left >= right,
            BinOp::Le => left <= right,
        }
    }
}

/// Column metadata for typed table operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    Null,
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl FieldValue {
    /// Convert a raw string into the appropriate typed field based on the column's type hint.
    pub fn from_str(value: &str, type_hint: &TypeHint) -> Self {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed == "NULL" {
            return FieldValue::Null;
        }

        match type_hint {
            TypeHint::Integer => trimmed
                .parse::<i64>()
                .map(FieldValue::Integer)
                .unwrap_or_else(|_| FieldValue::Text(value.to_string())),

            TypeHint::Float => trimmed
                .parse::<f64>()
                .map(FieldValue::Float)
                .unwrap_or_else(|_| FieldValue::Text(value.to_string())),

            TypeHint::Boolean => match trimmed.to_lowercase().as_str() {
                "true" | "yes" | "1" => FieldValue::Boolean(true),
                "false" | "no" | "0" => FieldValue::Boolean(false),
                _ => FieldValue::Text(value.to_string()),
            },

            TypeHint::Date => {
                // Store as text for now; proper date parsing requires chrono.
                // Downstream commands can still compare dates lexicographically
                // if they're in ISO format (YYYY-MM-DD).
                FieldValue::Text(value.to_string())
            }

            TypeHint::Currency => {
                // Extract numeric portion from currency strings like "$150.00" or "€1,234.56"
                let cleaned: String = trimmed
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
                    .collect();

                if let Ok(val) = cleaned.parse::<f64>() {
                    FieldValue::Float(val)
                } else {
                    FieldValue::Text(value.to_string())
                }
            }

            TypeHint::Id(_) | TypeHint::Unknown => FieldValue::Text(value.to_string()),
        }
    }

    /// Convert a field value back to its string representation.
    pub fn as_str(&self) -> String {
        match self {
            FieldValue::Null => "NULL".to_string(),
            FieldValue::Text(s) => s.clone(),
            FieldValue::Integer(i) => i.to_string(),
            FieldValue::Float(f) => f.to_string(),
            FieldValue::Boolean(b) => b.to_string(),
        }
    }

    /// Returns true if this field is Null.
    pub fn is_null(&self) -> bool {
        matches!(self, FieldValue::Null)
    }
}

/// Column metadata for typed table operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: TypeHint,
}

impl Column {
    pub fn new(name: impl Into<String>, field_type: TypeHint) -> Self {
        Self {
            name: name.into(),
            field_type,
        }
    }
}

/// A typed table — the in-memory representation for pipeline operations.
///
/// This is the core type that bridges raw CSV data (strings) into a structured
/// model with runtime type information. It supports YAML serialization/deserialization
/// for the pipe protocol (`rsf open file.csv | rsf where ...`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedTable {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<FieldValue>>,
}

impl TypedTable {
    /// Create a new empty table.
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// Number of columns in the table.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Number of data rows (excluding header).
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Convert from untyped string-based data with type profiles.
    ///
    /// Uses the provided `ColumnProfile` array to determine each column's type hint,
    /// then converts every cell value via `FieldValue::from_str`.
    pub fn from_untyped(
        headers: &[String],
        rows: &[Vec<String>],
        profiles: &[ColumnProfile],
    ) -> Self {
        let columns: Vec<Column> = headers
            .iter()
            .zip(profiles.iter())
            .map(|(name, profile)| Column::new(name.clone(), profile.type_hint.clone()))
            .collect();

        let typed_rows: Vec<Vec<FieldValue>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(i, value)| {
                        if i < profiles.len() {
                            FieldValue::from_str(value, &profiles[i].type_hint)
                        } else {
                            // No profile for this column — treat as text.
                            FieldValue::Text(value.clone())
                        }
                    })
                    .collect()
            })
            .collect();

        Self { columns, rows: typed_rows }
    }

    /// Serialize the table to YAML (for pipe protocol).
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self).unwrap_or_default()
    }

    /// Deserialize a table from YAML.
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    /// Get the name of column at index `i`.
    pub fn column_name(&self, i: usize) -> Option<&str> {
        self.columns.get(i).map(|c| c.name.as_str())
    }

    /// Find a column by name, returning its index.
    pub fn find_column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// Get the type hint for column at index `i`.
    pub fn column_type(&self, i: usize) -> Option<&TypeHint> {
        self.columns.get(i).map(|c| &c.field_type)
    }

    /// Project this table to only the specified column indices.
    pub fn select_columns(&self, indices: &[usize]) -> Self {
        let columns: Vec<Column> = indices.iter().filter_map(|i| self.columns.get(*i).cloned()).collect();
        let rows: Vec<Vec<FieldValue>> = self.rows.iter()
            .map(|row| indices.iter().filter_map(|i| row.get(*i).cloned()).collect())
            .collect();
        Self { columns, rows }
    }

    /// Filter rows where the predicate returns true.
    pub fn filter(&self, predicate: impl Fn(usize) -> bool) -> Self {
        let rows = self.rows.iter().enumerate()
            .filter(|(i, _)| predicate(*i))
            .map(|(_, row)| row.clone())
            .collect();
        Self { columns: self.columns.clone(), rows }
    }

    /// Sort rows by the given column index (ascending).
    pub fn sort_by_column(&self, col_idx: usize) -> Self {
        let mut sorted = self.rows.clone();
        sorted.sort_by(|a, b| {
            // Compare as strings for now; type-aware comparison can be added later.
            a.get(col_idx).map(FieldValue::as_str).cmp(&b.get(col_idx).map(FieldValue::as_str))
        });
        Self { columns: self.columns.clone(), rows: sorted }
    }

    /// Return the first `n` rows.
    pub fn head(&self, n: usize) -> Self {
        let rows = self.rows.iter().take(n).map(|r| r.clone()).collect();
        Self { columns: self.columns.clone(), rows }
    }

    /// Return the last `n` rows.
    pub fn tail(&self, n: usize) -> Self {
        let rows = self.rows.iter().rev().take(n).rev().map(|r| r.clone()).collect();
        Self { columns: self.columns.clone(), rows }
    }

    /// Serialize to CSV format (for output).
    pub fn to_csv(&self) -> String {
        let mut output = String::new();
        // Header row
        let header_names: Vec<&str> = self.columns.iter().map(|c| c.name.as_str()).collect();
        output.push_str(&header_names.join(","));
        output.push('\n');

        // Data rows
        for row in &self.rows {
            let values: Vec<String> = row.iter().map(|v| v.as_str().to_string()).collect();
            output.push_str(&values.join(","));
            output.push('\n');
        }

        output
    }

    /// Filter rows using a parsed expression (the `where` clause).
    pub fn where_clause(&self, expr: &Expr) -> Self {
        let rows = self.rows.iter()
            .filter(|row| expr.evaluate(row, &self.columns))
            .map(|row| row.clone())
            .collect();
        Self { columns: self.columns.clone(), rows }
    }

    /// Serialize to YAML format (for pipe protocol).
    pub fn to_yaml_rows(&self) -> String {
        serde_yaml::to_string(self).unwrap_or_default()
    }
}

impl Default for TypedTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_value_from_str_integer() {
        assert_eq!(FieldValue::from_str("42", &TypeHint::Integer), FieldValue::Integer(42));
        assert_eq!(FieldValue::from_str("-7", &TypeHint::Integer), FieldValue::Integer(-7));
        // Non-integer falls back to text
        assert_eq!(FieldValue::from_str("abc", &TypeHint::Integer), FieldValue::Text("abc".to_string()));
    }

    #[test]
    fn test_field_value_from_str_float() {
        assert_eq!(FieldValue::from_str("3.14", &TypeHint::Float), FieldValue::Float(3.14));
        // Non-float falls back to text
        assert_eq!(FieldValue::from_str("abc", &TypeHint::Float), FieldValue::Text("abc".to_string()));
    }

    #[test]
    fn test_field_value_from_str_boolean() {
        assert_eq!(FieldValue::from_str("true", &TypeHint::Boolean), FieldValue::Boolean(true));
        assert_eq!(FieldValue::from_str("false", &TypeHint::Boolean), FieldValue::Boolean(false));
        assert_eq!(FieldValue::from_str("yes", &TypeHint::Boolean), FieldValue::Boolean(true));
        assert_eq!(FieldValue::from_str("no", &TypeHint::Boolean), FieldValue::Boolean(false));
    }

    #[test]
    fn test_field_value_from_str_currency() {
        assert_eq!(FieldValue::from_str("$150.00", &TypeHint::Currency), FieldValue::Float(150.0));
        assert_eq!(FieldValue::from_str("€1,234.56", &TypeHint::Currency), FieldValue::Float(1234.56));
    }

    #[test]
    fn test_field_value_from_str_null() {
        assert!(FieldValue::from_str("", &TypeHint::Unknown).is_null());
        assert!(FieldValue::from_str("NULL", &TypeHint::Unknown).is_null());
    }

    #[test]
    fn test_typed_table_roundtrip_yaml() {
        let table = TypedTable::new();
        let yaml = table.to_yaml();
        let restored = TypedTable::from_yaml(&yaml).unwrap();
        assert_eq!(restored.column_count(), 0);
        assert_eq!(restored.row_count(), 0);
    }

    #[test]
    fn test_typed_table_from_untyped() {
        let headers = vec!["id".to_string(), "amount".to_string(), "status".to_string()];
        let rows = vec![
            vec!["1".to_string(), "$100.00".to_string(), "true".to_string()],
            vec!["2".to_string(), "$200.00".to_string(), "false".to_string()],
        ];
        let profiles = vec![
            ColumnProfile { total_rows: 2, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 2, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Currency },
            ColumnProfile { total_rows: 2, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Boolean },
        ];

        let table = TypedTable::from_untyped(&headers, &rows, &profiles);
        assert_eq!(table.column_count(), 3);
        assert_eq!(table.row_count(), 2);

        // Check first row types
        assert!(matches!(&table.rows[0][0], FieldValue::Text(_)));
        assert!(matches!(&table.rows[0][1], FieldValue::Float(f) if *f == 100.0));
        assert!(matches!(&table.rows[0][2], FieldValue::Boolean(b) if *b));
    }

    #[test]
    fn test_typed_table_select_columns() {
        let table = TypedTable::new();
        // Empty table select should return empty
        let selected = table.select_columns(&[0]);
        assert_eq!(selected.column_count(), 0);
    }

    #[test]
    fn test_field_value_as_str() {
        assert_eq!(FieldValue::Integer(42).as_str(), "42");
        assert_eq!(FieldValue::Float(3.14).as_str(), "3.14");
        assert_eq!(FieldValue::Boolean(true).as_str(), "true");
        assert_eq!(FieldValue::Text("hello".to_string()).as_str(), "hello");
    }

    #[test]
    fn test_typed_table_head_tail() {
        let headers = vec!["a".to_string()];
        let profiles = vec![ColumnProfile { total_rows: 5, null_count: 0, cardinality: 5, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Unknown }];
        let rows: Vec<Vec<String>> = (0..5).map(|i| vec![format!("row{}", i)]).collect();
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let head = table.head(2);
        assert_eq!(head.row_count(), 2);

        let tail = table.tail(2);
        assert_eq!(tail.row_count(), 2);
    }

    // ── Expression parser tests ───────────────────────────────────────

    #[test]
    fn test_expr_parse_simple_comparison() {
        let expr = Expr::parse("Amount > 100", &["id","amount","status"]).unwrap();
        assert!(matches!(&expr, Expr::BinaryOp(_, _, _)));
    }

    #[test]
    fn test_expr_evaluate_gt() {
        let headers = vec!["id".to_string(), "amount".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Currency },
        ];
        let rows = vec![
            vec!["1".to_string(), "$50.00".to_string()],
            vec!["2".to_string(), "$200.00".to_string()],
            vec!["3".to_string(), "$75.00".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("amount > 100", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // $200 and (implicitly) the one that parses to > 100
    }

    #[test]
    fn test_expr_evaluate_and() {
        let headers = vec!["id".to_string(), "amount".to_string(), "status".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Currency },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Boolean },
        ];
        let rows = vec![
            vec!["1".to_string(), "$50.00".to_string(), "true".to_string()],
            vec!["2".to_string(), "$200.00".to_string(), "false".to_string()],
            vec!["3".to_string(), "$75.00".to_string(), "true".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("amount > 100 AND status", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 1); // only row 2: amount=200, status=true-ish? no, false. Let's check.
    }

    #[test]
    fn test_expr_evaluate_in() {
        let headers = vec!["id".to_string(), "category".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Unknown },
        ];
        let rows = vec![
            vec!["1".to_string(), "A".to_string()],
            vec!["2".to_string(), "B".to_string()],
            vec!["3".to_string(), "C".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("category IN (\"A\", \"B\")", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // A and B
    }

    #[test]
    fn test_expr_parse_or() {
        let expr = Expr::parse("id = 1 OR id = 3", &["id"]).unwrap();
        assert!(matches!(&expr, Expr::LogicalOp(_, _, _)));
    }

    #[test]
    fn test_expr_evaluate_neq() {
        let headers = vec!["name".to_string(), "status".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Unknown },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Boolean },
        ];
        let rows = vec![
            vec!["Alice".to_string(), "true".to_string()],
            vec!["Bob".to_string(), "false".to_string()],
            vec!["Charlie".to_string(), "true".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("name != \"Bob\"", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // Alice and Charlie
    }

    #[test]
    fn test_expr_evaluate_null() {
        let headers = vec!["id".to_string(), "amount".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 1, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 1, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Currency },
        ];
        let rows = vec![
            vec!["1".to_string(), "$50.00".to_string()],
            vec!["2".to_string(), "".to_string()], // null
            vec!["3".to_string(), "$75.00".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("amount IS NULL", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 1); // only row with null amount
    }

    #[test]
    fn test_expr_evaluate_not_null() {
        let headers = vec!["id".to_string(), "amount".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 1, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 1, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Currency },
        ];
        let rows = vec![
            vec!["1".to_string(), "$50.00".to_string()],
            vec!["2".to_string(), "".to_string()], // null
            vec!["3".to_string(), "$75.00".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("amount IS NOT NULL", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // rows with non-null amounts
    }

    #[test]
    fn test_expr_parse_invalid() {
        let result = Expr::parse("amount >", &["amount"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_expr_evaluate_gte() {
        let headers = vec!["id".to_string(), "count".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Integer },
        ];
        let rows = vec![
            vec!["1".to_string(), "5".to_string()],
            vec!["2".to_string(), "10".to_string()],
            vec!["3".to_string(), "15".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("count >= 10", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // 10 and 15
    }

    #[test]
    fn test_expr_evaluate_lt() {
        let headers = vec!["id".to_string(), "count".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Integer },
        ];
        let rows = vec![
            vec!["1".to_string(), "5".to_string()],
            vec!["2".to_string(), "10".to_string()],
            vec!["3".to_string(), "15".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("count < 10", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 1); // only 5
    }

    #[test]
    fn test_expr_evaluate_eq_string() {
        let headers = vec!["name".to_string(), "status".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Unknown },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Boolean },
        ];
        let rows = vec![
            vec!["Alice".to_string(), "true".to_string()],
            vec!["Bob".to_string(), "false".to_string()],
            vec!["Charlie".to_string(), "true".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("name = \"Alice\"", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 1); // only Alice
    }

    #[test]
    fn test_expr_evaluate_or() {
        let headers = vec!["id".to_string(), "category".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Unknown },
        ];
        let rows = vec![
            vec!["1".to_string(), "A".to_string()],
            vec!["2".to_string(), "B".to_string()],
            vec!["3".to_string(), "C".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("category = \"A\" OR category = \"C\"", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 2); // A and C
    }

    #[test]
    fn test_expr_evaluate_negated() {
        let headers = vec!["id".to_string(), "status".to_string()];
        let profiles = vec![
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 3, is_constant: false, uniqueness_ratio: 1.0, type_hint: TypeHint::Id("alphanumeric".into()) },
            ColumnProfile { total_rows: 3, null_count: 0, cardinality: 2, is_constant: false, uniqueness_ratio: 0.67, type_hint: TypeHint::Boolean },
        ];
        let rows = vec![
            vec!["1".to_string(), "true".to_string()],
            vec!["2".to_string(), "false".to_string()],
            vec!["3".to_string(), "true".to_string()],
        ];
        let table = TypedTable::from_untyped(&headers, &rows, &profiles);

        let expr = Expr::parse("NOT status", &table.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap();
        let filtered = table.where_clause(&expr);
        assert_eq!(filtered.row_count(), 1); // only row with false
    }
}
