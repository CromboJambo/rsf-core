//! Expression language for RSF table filtering.
//!
//! Provides tokenizer, parser, and evaluator for a SQL-like expression
//! language used in TypedTable.where_clause() and derived columns.

pub mod types;
pub mod lexer;
pub mod parser;
pub mod evaluator;
pub mod pest_parser;

// Re-export key types at module level for convenience
pub use types::{Token, Expr, BinOp, BinLogOp, ParseError};
pub use lexer::tokenize;
pub use parser::parse;
