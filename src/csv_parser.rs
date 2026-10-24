//! CSV parsing module - handles encoding detection, UTF-16/UTF-8 conversion



/// Detected text encoding
#[derive(Debug, Clone, PartialEq)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

/// A single field (column value) in a CSV row
#[derive(Debug, Clone)]
pub struct Field {
    pub value: String,
    raw_bytes: Vec<u8>,
}

impl Field {
    pub fn new(value: String) -> Self {
        let bytes = value.as_bytes().to_vec();
        Self { value, raw_bytes: bytes }
    }

    pub fn normalize(&mut self) {
        // Trim whitespace from both ends
        self.value = self.value.trim().to_string();
    }

    /// Get the raw byte representation (useful for encoding detection)
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

/// Parsed CSV table structure
#[derive(Debug)]
pub struct Table {
    header: Vec<Field>,
    rows: Vec<Vec<Field>>,
    encoding: Encoding,
}

impl Table {
    pub fn new(header: Vec<Field>, rows: Vec<Vec<Field>>, encoding: Encoding) -> Self {
        Self { header, rows, encoding }
    }

    pub fn column_count(&self) -> usize {
        self.header.len()
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Verify all rows have consistent field count (matches header)
    pub fn validate_structure(&self) -> anyhow::Result<()> {
        let expected = self.column_count();
        for (i, row) in self.rows.iter().enumerate() {
            if row.len() != expected {
                return Err(anyhow::anyhow!(
                    "Row {} has {} fields, expected {}",
                    i + 1,
                    row.len(),
                    expected
                ));
            }
        }
        Ok(())
    }

    /// Print a summary of the table (for debugging)
    pub fn print_summary(&self) {
        println!("┌─────────────────────── Table Summary ────────────────────────┐");
        println!("│  Encoding: {:<34} │", format!("{:?}:", self.encoding));
        println!("│  Columns: {}{:<29} │", self.column_count(), "");
        println!("│  Rows: {},{:<35} │", self.row_count(), "");

        let inconsistent_rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.len() != self.column_count())
            .collect();

        if inconsistent_rows.is_empty() {
            println!("│  ✓ All rows have consistent field counts{:<30} │", "");
        } else {
            println!(
                "│  ✗ {} rows with inconsistent field counts{:<18} │",
                inconsistent_rows.len(),
                ""
            );
            for (i, row) in &inconsistent_rows[..inconsistent_rows.len().min(5)] {
                println!(
                    "    Row {}: expected {}, got {}",
                    i + 1,
                    self.column_count(),
                    row.len()
                );
            }
        }
        println!("└──────────────────────────────────────────────────────────────┘");
    }

    /// Get header fields
    pub fn headers(&self) -> &Vec<Field> {
        &self.header
    }

    /// Get all rows (excluding header)
    pub fn rows(&self) -> &Vec<Vec<Field>> {
        &self.rows
    }

    /// Get encoding used
    pub fn encoding(&self) -> Encoding {
        self.encoding.clone()
    }
}

/// Detect text encoding from raw bytes
fn detect_encoding(raw: &[u8]) -> Encoding {
    // Check for BOM (Byte Order Mark)
    if raw.starts_with(&[0xFF, 0xFE]) {
        return Encoding::Utf16Le;
    }
    if raw.starts_with(&[0xFE, 0xFF]) {
        return Encoding::Utf16Be;
    }

    // Sample first few bytes to detect ASCII vs UTF-8
    let sample = &raw[..raw.len().min(10_000)];
    let ascii_ratio = sample
        .iter()
        .filter(|&&b| b < 128 || b == 0x0D || b == 0x0A)
        .count() as f64
        / sample.len() as f64;

    if ascii_ratio > 0.95 {
        Encoding::Utf8
    } else {
        // Assume UTF-8 for non-ASCII content (most common case)
        Encoding::Utf8
    }
}

/// Parse a CSV row, handling quoted fields and embedded delimiters
fn parse_csv_row(line: &str) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = line.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        match *c {
            '"' => {
                if in_quotes && i + 1 < chars.len() && chars[i + 1] == '"' {
                    // Escaped quote: add single quote and skip next char
                    current_field.push('"');
                } else {
                    // Toggle quote state
                    in_quotes = !in_quotes;
                }
            }
            '\t' | ',' if !in_quotes => {
                // Delimiter outside quotes = field boundary
                fields.push(Field::new(current_field.clone()));
                current_field.clear();
            }
            _ => {
                current_field.push(*c);
            }
        }
    }

    // Add last field
    if !current_field.is_empty() || in_quotes {
        fields.push(Field::new(current_field));
    } else if chars.len() > 0 && (chars.last().unwrap() == &'\t' || chars.last().unwrap() == &',') {
        // Trailing delimiter = empty field at end
        fields.push(Field::new(String::new()));
    }

    fields
}

/// Handle UTF-16 LE encoding
fn decode_utf16_le(raw: &[u8]) -> String {
    let data = if raw.starts_with(&[0xFF, 0xFE]) { &raw[2..] } else { raw };
    let mut chars = Vec::new();
    for chunk in data.chunks(2) {
        if chunk.len() == 2 {
            let c = u16::from_le_bytes([chunk[0], chunk[1]]);
            if let Some(c) = char::from_u32(c as u32) {
                chars.push(c);
            }
        }
    }
    String::from_iter(chars)
}

/// Handle UTF-16 BE encoding
fn decode_utf16_be(raw: &[u8]) -> String {
    let data = if raw.starts_with(&[0xFE, 0xFF]) { &raw[2..] } else { raw };
    let mut chars = Vec::new();
    for chunk in data.chunks(2) {
        if chunk.len() == 2 {
            let c = u16::from_be_bytes([chunk[0], chunk[1]]);
            if let Some(c) = char::from_u32(c as u32) {
                chars.push(c);
            }
        }
    }
    String::from_iter(chars)
}

/// Load and parse a CSV file, detecting encoding automatically
pub fn load_csv(path: &std::path::PathBuf) -> anyhow::Result<Table> {
    let raw_bytes = std::fs::read(path)?;
    let encoding = detect_encoding(&raw_bytes);

    // Decode to UTF-8 string
    let decoded = match encoding {
        Encoding::Utf16Le => decode_utf16_le(&raw_bytes),
        Encoding::Utf16Be => decode_utf16_be(&raw_bytes),
        Encoding::Utf8 => String::from_utf8_lossy(&raw_bytes).to_string(),
    };

    // Split into lines (handle various line endings)
    let mut lines = Vec::new();
    for line in decoded.lines() {
        if !line.is_empty() || !lines.is_empty() {
            lines.push(line.to_string());
        }
    }

    if lines.is_empty() {
        return Err(anyhow::anyhow!("Empty CSV file"));
    }

    // Parse header
    let header = parse_csv_row(&lines[0]);

    // Handle embedded newlines in quoted fields (merge continuation lines)
    let mut result_lines: Vec<String> = Vec::new();
    let mut i = 1;

    while i < lines.len() {
        let current = &lines[i]; // Use &str instead of String

        if i + 1 < lines.len() && is_header_continuation(current, &lines[i + 1]) {
            // Merge lines with embedded newlines
            let merged = merge_lines_with_embedded_newline(current, &lines[i + 1]);
            result_lines.push(merged);
            i += 2;
        } else {
            result_lines.push(current.clone());
            i += 1;
        }
    }

    // Parse all rows (header was already parsed separately)
    let rows: Vec<Vec<Field>> = result_lines
        .iter()
        .map(|line| parse_csv_row(line))
        .collect();

    Ok(Table::new(header, rows, encoding.clone()))
}

/// Check if line2 is a continuation of line1 (e.g., unquoted text split by newline)
fn is_header_continuation(line1: &str, line2: &str) -> bool {
    let trimmed1 = line1.trim();
    let trimmed2 = line2.trim_start();

    // Only merge if line1 has an unbalanced opening quote (embedded newline in quoted field)
    let open_quotes = trimmed1.chars().filter(|&c| c == '"').count();
    open_quotes % 2 == 1
}

/// Merge two lines that were split by an embedded newline in a quoted field
fn merge_lines_with_embedded_newline(line1: &str, line2: &str) -> String {
    let trimmed1 = line1.trim();
    let trimmed2 = line2.trim_start();

    if trimmed1.ends_with('"') && trimmed2.starts_with('"') {
        format!(
            "{}{}",
            &trimmed1[..trimmed1.len() - 1],
            &trimmed2[1..]
        )
    } else {
        format!("{} {}", trimmed1, trimmed2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoding_detection_utf8() {
        let raw = b"Hello,World".to_vec();
        assert_eq!(detect_encoding(&raw), Encoding::Utf8);
    }

    #[test]
    fn test_encoding_detection_utf16le_bom() {
        let raw = vec![0xFF, 0xFE, b'H', 0x00, b'e', 0x00, b'l', 0x00, b'l', 0x00, b'o', 0x00];
        assert_eq!(detect_encoding(&raw), Encoding::Utf16Le);
    }

    #[test]
    fn test_utf16le_decode() {
        let raw = vec![0xFF, 0xFE, b'H', 0x00, b'e', 0x00, b'l', 0x00, b'l', 0x00, b'o', 0x00];
        let decoded = decode_utf16_le(&raw);
        assert_eq!(decoded, "Hello");
    }

    #[test]
    fn test_csv_row_parsing() {
        let line = "\"Order\"\t\"CO Description\"\t\"Customer\"";
        let fields = parse_csv_row(line);

        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].value, "Order");
        assert_eq!(fields[1].value, "CO Description");
        assert_eq!(fields[2].value, "Customer");
    }
}
