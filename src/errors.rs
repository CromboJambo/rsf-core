/// Custom error type for RSF operations (retained for display purposes)
#[derive(Debug)]
pub enum RsfError {
    /// File I/O error
    IoError { path: std::path::PathBuf, cause: std::io::Error },
    /// CSV parsing error
    CsvError { message: String },
    /// Schema validation error
    SchemaError { message: String },
    /// Invalid column ordering
    ColumnOrderError { position: usize, expected: String, found: String },
    /// Invalid cardinality ranking
    CardinalityError { column: String, expected: usize, found: usize },
    /// Row sorting error
    SortError,
    /// Unknown error type
    Unknown(String),
}

impl std::fmt::Display for RsfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RsfError::IoError { path, cause } => write!(f, "Failed to open file '{}': {}", path.display(), cause),
            RsfError::CsvError { message } => write!(f, "CSV error: {}", message),
            RsfError::SchemaError { message } => write!(f, "Schema error: {}", message),
            RsfError::ColumnOrderError { position, expected, found } => write!(f, "Column order mismatch at position {}: expected '{}', found '{}'", position, expected, found),
            RsfError::CardinalityError { column, expected, found } => write!(f, "Column '{}' has invalid cardinality: expected {}, found {}", column, expected, found),
            RsfError::SortError => write!(f, "Rows are not in canonical sorted order"),
            RsfError::Unknown(message) => write!(f, "Unknown error: {}", message),
        }
    }
}

impl std::error::Error for RsfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RsfError::IoError { cause, .. } => Some(cause),
            _ => None,
        }
    }
}

impl From<std::io::Error> for RsfError {
    fn from(err: std::io::Error) -> Self {
        RsfError::IoError { path: std::path::PathBuf::from("<unknown>"), cause: err }
    }
}

/// Result type alias - uses anyhow for cross-crate compatibility
pub type RsfResult<T> = anyhow::Result<T>;