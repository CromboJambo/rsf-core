//! Expression language types: tokens and AST nodes.

use crate::table::{Column, FieldValue};
use std::fmt;

/// Token types for the expression tokenizer.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    StringLit(String),
    Number(f64),
    Bool(bool),
    Null,
    // Comparison operators
    Eq,
    Neq,
    Gt,
    Lt,
    Ge,
    Le,
    // Arithmetic operators
    Plus,
    Minus,
    Star,
    Slash,
    // Logical operators (keywords)
    And,
    Or,
    Not,
    Is,
    In,
    // Punctuation
    LParen,
    RParen,
    Comma,
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

/// An expression AST node.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Column reference by name (e.g., `amount`)
    Column(String),
    /// A literal value
    Literal(FieldValue),
    /// Binary operator: left OP right (comparison or arithmetic)
    BinaryOp(Box<Expr>, BinOp, Box<Expr>),
    /// Logical AND/OR between two expressions
    LogicalOp(BinLogOp, Box<Expr>, Box<Expr>),
    /// Unary NOT
    Not(Box<Expr>),
    /// Unary minus (arithmetic negation)
    Negate(Box<Expr>),
    /// IS NULL / IS NOT NULL
    NullCheck(String, bool), // (column_name, is_not)
    /// IN (val1, val2, ...)
    In(String, Vec<FieldValue>),
}

/// Binary operators: comparisons (`Eq`..`Le`) and arithmetic
/// (`Add`/`Sub`/`Mul`/`Div`).
#[derive(Debug, Clone)]
pub enum BinOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Ge,
    Le,
    Add,
    Sub,
    Mul,
    Div,
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
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
        }
    }
}

/// Binary logical operators.
#[derive(Debug, Clone)]
pub enum BinLogOp {
    And,
    Or,
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
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Parse error: {}", self.0)
    }
}

impl std::error::Error for ParseError {}
