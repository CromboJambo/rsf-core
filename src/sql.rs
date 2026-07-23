//! SQL query layer over RSF data using Turso as the backend.
//!
//! This module provides a thin wrapper around Turso's async API, exposing
//! functionality to register tables and execute queries against in-memory
//! RSF data structures.

use std::sync::Arc;

/// A single row of SQL result data.
#[derive(Debug, Clone)]
pub struct Row {
    pub columns: Vec<String>,
    pub values: Vec<serde_json::Value>,
}

impl Row {
    /// Get a value from this row by column name.
    #[allow(dead_code)] // Used in tests
    pub fn get(&self, column: &str) -> Option<&serde_json::Value> {
        self.columns.iter().position(|c| c == column).and_then(|i| self.values.get(i))
    }
}

/// A set of rows returned from a query.
#[derive(Debug, Clone)]
pub struct Rows {
    pub inner: Vec<Row>,
}

impl Rows {
    /// Consume the rows and return them as a vector of `Row`.
    #[allow(dead_code)] // Used in tests
    pub fn into_rows(self) -> Vec<Row> {
        self.inner
    }

    /// Return an iterator over the rows.
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.inner.iter()
    }

    /// Get the number of rows.
    #[allow(dead_code)] // Used in tests
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if there are no rows.
    #[allow(dead_code)] // Used in tests
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// A table definition with column names and types.
#[derive(Debug, Clone)]
pub struct TableDefinition {
    /// Name of the table
    #[allow(dead_code)] // Used in tests
    pub name: String,
    /// Column metadata from the RSF ranking system (if available)
    #[allow(dead_code)] // Used in tests
    pub columns: Vec<String>,
}

/// The main database handle for executing queries against registered tables.
#[derive(Debug, Default)]
pub struct Database {
    /// Internal state managed by Turso
    inner: Arc<tokio::sync::Mutex<Option<Inner>>>,
}

struct Inner {
    conn: turso::Connection<'static>,
}

/// A prepared statement for parameterized queries.
#[derive(Debug, Clone)]
pub struct Statement {
    pub(crate) inner: std::sync::Arc<std::sync::Mutex<turso::Statement<'static>>>,
}

impl Database {
    /// Create a new in-memory database instance.
    #[allow(dead_code)] // Used in tests
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = turso::Database::open_local("file::memory:", &turso::Config::default()).await?;
        let conn = db.connect()?;

        Ok(Self {
            inner: Arc::new(tokio::sync::Mutex::new(Some(Inner { conn }))),
        })
    }

    /// Register a table with the database.
    #[allow(dead_code)] // Used in tests
    pub async fn register_table<T>(&self, name: &str, columns: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut guard = self.inner.lock().await;

        if let Some(inner) = guard.as_ref() {
            // Create the table in Turso
            inner.conn.execute(format!("CREATE TABLE IF NOT EXISTS {} ({})", name, columns.join(", ")), ()).await?;
        }

        Ok(())
    }

    /// Execute a SQL query and return the results.
    #[allow(dead_code)] // Used in tests
    pub async fn query(&self, sql: &str) -> Result<Rows, Box<dyn std::error::Error + Send + Sync>> {
        let guard = self.inner.lock().await;

        if let Some(inner) = guard.as_ref() {
            // Execute the query and return results
            let mut stmt = inner.conn.prepare(sql).await?;
            let _rows = stmt.query(()).await?;

            Ok(Rows {
                inner: vec![], // TODO: Implement row mapping from Turso result set
            })
        } else {
            Err("Database not initialized".into())
        }
    }

    /// Close the database connection.
    #[allow(dead_code)] // Used in tests
    pub async fn close(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut guard = self.inner.lock().await;

        if let Some(inner) = guard.take() {
            drop(guard); // Release lock before dropping inner
            drop(inner);  // Close connection
        }

        Ok(())
    }
}

impl std::fmt::Display for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Database")
    }
}

/// A trait for converting RSF column metadata to SQL types.
pub trait ToSqlType {
    /// Convert the type information to a SQL data type string.
    fn sql_type(&self) -> &'static str;
}

impl ToSqlType for crate::ColumnMeta {
    fn sql_type(&self) -> &'static str {
        // Default mapping based on detected type hint
        match &self.type_hint {
            Some(crate::TypeHint::Integer) => "INTEGER",
            Some(crate::TypeHint::Float) => "REAL",
            Some(crate::TypeHint::Boolean) => "BOOLEAN",
            _ => "TEXT", // Default to TEXT for unknown types
        }
    }
}

/// A trait for converting RSF rows to database values.
pub trait ToSqlValue {
    /// Convert the value to a SQL parameter type.
    fn sql_value(&self) -> turso::types::Value;
}

impl<T> ToSqlValue for T 
where
    T: std::fmt::Display,
{
    fn sql_value(&self) -> turso::types::Value {
        turso::types::Value::Text(self.to_string())
    }
}

/// A builder pattern helper for constructing complex queries.
#[derive(Debug, Default)]
pub struct QueryBuilder<'a> {
    select: Option<String>,
    from: Option<&'a str>,
    r#where: Option<(String, Vec<tokio::sync::Mutex<Option<()>>>)>, // Simplified for now
}

impl<'a> QueryBuilder<'a> {
    /// Create a new query builder.
    #[allow(dead_code)] // Used in tests
    pub fn new() -> Self {
        Self {
            select: None,
            from: None,
            r#where: None,
        }
    }

    /// Set the SELECT clause.
    #[allow(dead_code)] // Used in tests
    pub fn select(mut self, columns: &[&str]) -> Self {
        self.select = Some(columns.join(", "));
        self
    }

    /// Set the FROM clause.
    #[allow(dead_code)] // Used in tests
    pub fn from_table(mut self, table: &'a str) -> Self {
        self.from = Some(table);
        self
    }

    /// Add a WHERE condition.
    #[allow(dead_code)] // Used in tests
    pub fn r#where<T: ToSqlValue>(mut self, column: &str, op: &str, _value: T) -> Self {
        let params = vec![];
        self.r#where = Some((format!("{} {} ?", column, op), params));
        self
    }

    /// Build and return the final SQL query string.
    #[allow(dead_code)] // Used in tests
    pub fn build(self) -> String {
        let select = self.select.unwrap_or_else(|| "*".to_string());
        let from = self.from.unwrap_or("unknown_table");

        let mut sql = format!("SELECT {} FROM {}", select, from);

        if let Some((where_clause, _)) = &self.r#where {
            sql.push_str(&format!(" WHERE {}", where_clause));
        }

        sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_creation() {
        let db = Database::new().await.unwrap();
        assert!(db.to_string().contains("Database"));
    }

    #[test]
    fn test_query_builder() {
        let builder = QueryBuilder::new()
            .select(&["col1", "col2"])
            .from_table("my_table")
            .r#where("status", "=", "active");

        let sql = builder.build();
        assert!(sql.contains("SELECT col1, col2"));
        assert!(sql.contains("FROM my_table"));
        assert!(sql.contains("WHERE status = ?"));
    }

    #[test]
    fn test_to_sql_type_mapping() {
        use crate::ColumnMeta;

        let int_col = ColumnMeta {
            name: "id".to_string(),
            r#type: Some(crate::ColumnType::Integer),
            ..Default::default()
        };

        assert_eq!(int_col.sql_type(), "INTEGER");
    }
}
