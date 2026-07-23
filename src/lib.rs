//! **rsf** (Ranked Spreadsheet Format) - Semantic model for tabular data
//! 
//! Core abstraction layer that understands:
//! - Table structure (headers, rows, columns)
//! - Column semantics (cardinality, type hints, uniqueness)  
//! - Canonical serialization (deterministic ordering)
//!
//! No knowledge of Git, Excel, or CLI — pure semantic model.

pub mod csv_parser;
pub mod dedup;
pub mod errors;
pub mod ranking;
pub mod table;

pub use csv_parser::*;
pub use dedup::*;
pub use ranking::*;
pub use table::*;
