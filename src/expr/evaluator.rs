//! Expression evaluator for RSF filter expressions.

use crate::expr::types::{BinLogOp, BinOp, Expr, ParseError};
use crate::table::{Column, FieldValue};

/// Evaluate a parsed expression against a row of values, returning true/false.
pub fn evaluate(expr: &Expr, row: &[FieldValue], columns: &[Column]) -> bool {
    match expr {
        Expr::BinaryOp(left, op, right) => {
            // Comparison or arithmetic used as predicate
            if is_comparison_op(op) {
                eval_binary_op(expr, left, op, right, row, columns)
            } else {
                // Arithmetic expression used as predicate - check truthiness of result
                let val = resolve_value(expr, expr, row, columns);
                value_is_truthy(&val)
            }
        }
        Expr::LogicalOp(op, left, right) => match op {
            BinLogOp::And => evaluate(left, row, columns) && evaluate(right, row, columns),
            BinLogOp::Or => evaluate(left, row, columns) || evaluate(right, row, columns),
        },
        Expr::Not(inner) => !evaluate(inner, row, columns),
        Expr::NullCheck(name, is_not) => {
            let idx = column_index(columns, name);
            match idx {
                Some(i) => match &row[i] {
                    FieldValue::Null => *is_not == false,
                    _ => *is_not == true,
                },
                None => false,
            }
        }
        Expr::In(name, values) => {
            let idx = column_index(columns, name);
            if let Some(i) = idx {
                let cell = &row[i];
                values.iter().any(|v| match (cell, v) {
                    (FieldValue::Integer(a), FieldValue::Integer(b)) => a == b,
                    (FieldValue::Float(a), FieldValue::Float(b)) => a == b,
                    (FieldValue::Text(a), FieldValue::Text(b)) => a == b,
                    _ => false,
                })
            } else {
                false
            }
        }
        _ => {
            // Column reference or literal used as predicate - check truthiness
            let val = resolve_value(expr, expr, row, columns);
            value_is_truthy(&val)
        }
    }
}

fn is_comparison_op(op: &BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge)
}

fn value_is_truthy(val: &FieldValue) -> bool {
    match val {
        FieldValue::Null => false,
        FieldValue::Boolean(b) => *b,
        FieldValue::Integer(n) => *n != 0,
        FieldValue::Float(f) => *f != 0.0,
        FieldValue::Text(s) => !s.is_empty(),
    }
}

fn eval_binary_op(expr: &Expr, left: &Expr, op: &BinOp, right: &Expr, row: &[FieldValue], columns: &[Column]) -> bool {
    let lval = resolve_value(expr, left, row, columns);
    let rval = resolve_value(expr, right, row, columns);

    match op {
        BinOp::Eq => values_equal(&lval, &rval),
        BinOp::Neq => !values_equal(&lval, &rval),
        BinOp::Lt => compare_values(&lval, &rval) < 0,
        BinOp::Gt => compare_values(&lval, &rval) > 0,
        BinOp::Le => compare_values(&lval, &rval) <= 0,
        BinOp::Ge => compare_values(&lval, &rval) >= 0,
        _ => false,
    }
}

fn values_equal(a: &FieldValue, b: &FieldValue) -> bool {
    match (a, b) {
        (FieldValue::Integer(x), FieldValue::Integer(y)) => x == y,
        (FieldValue::Float(x), FieldValue::Float(y)) => x == y,
        (FieldValue::Text(x), FieldValue::Text(y)) => x == y,
        (FieldValue::Boolean(x), FieldValue::Boolean(y)) => x == y,
        (FieldValue::Null, FieldValue::Null) => true,
        _ => false,
    }
}

fn compare_values(a: &FieldValue, b: &FieldValue) -> i32 {
    match (a, b) {
        (FieldValue::Integer(x), FieldValue::Integer(y)) => x.cmp(y) as i32,
        (FieldValue::Float(x), FieldValue::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal) as i32,
        (FieldValue::Text(x), FieldValue::Text(y)) => x.cmp(y) as i32,
        // Cross-type numeric comparisons
        (FieldValue::Integer(x), FieldValue::Float(y)) => {
            (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal) as i32
        }
        (FieldValue::Float(x), FieldValue::Integer(y)) => {
            x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal) as i32
        }
        // Text vs numeric: try to parse text as number
        (FieldValue::Text(x), FieldValue::Integer(y)) => {
            if let Ok(num) = x.parse::<f64>() {
                num.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
        (FieldValue::Integer(x), FieldValue::Text(y)) => {
            if let Ok(num) = y.parse::<f64>() {
                (*x as f64).partial_cmp(&num).unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
        (FieldValue::Text(x), FieldValue::Float(y)) => {
            if let Ok(num) = x.parse::<f64>() {
                num.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
        (FieldValue::Float(x), FieldValue::Text(y)) => {
            if let Ok(num) = y.parse::<f64>() {
                x.partial_cmp(&num).unwrap_or(std::cmp::Ordering::Equal) as i32
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn resolve_value(expr: &Expr, sub: &Expr, row: &[FieldValue], columns: &[Column]) -> FieldValue {
    match sub {
        Expr::Column(name) => {
            let idx = column_index(columns, name);
            if let Some(i) = idx {
                row[i].clone()
            } else {
                FieldValue::Null
            }
        }
        Expr::Literal(val) => val.clone(),
        Expr::BinaryOp(left, op, right) => {
            let lval = resolve_value(expr, left, row, columns);
            let rval = resolve_value(expr, right, row, columns);
            eval_arithmetic(op, &lval, &rval)
        }
        Expr::Negate(inner) => {
            let inner_val = resolve_value(expr, inner, row, columns);
            match inner_val {
                FieldValue::Integer(n) => FieldValue::Integer(-n),
                FieldValue::Float(f) => FieldValue::Float(-f),
                _ => FieldValue::Null,
            }
        }
        _ => FieldValue::Null,
    }
}

fn eval_arithmetic(op: &BinOp, left: &FieldValue, right: &FieldValue) -> FieldValue {
    match (left, right) {
        (FieldValue::Integer(l), FieldValue::Integer(r)) => {
            let result = match op {
                BinOp::Add => *l + *r,
                BinOp::Sub => *l - *r,
                BinOp::Mul => *l * *r,
                BinOp::Div => *l / *r,
                _ => return FieldValue::Null,
            };
            FieldValue::Integer(result)
        }
        (FieldValue::Float(l), FieldValue::Float(r)) => {
            let result = match op {
                BinOp::Add => *l + *r,
                BinOp::Sub => *l - *r,
                BinOp::Mul => *l * *r,
                BinOp::Div => *l / *r,
                _ => return FieldValue::Null,
            };
            FieldValue::Float(result)
        }
        (FieldValue::Integer(l), FieldValue::Float(r)) => {
            let result = match op {
                BinOp::Add => (*l as f64) + *r,
                BinOp::Sub => (*l as f64) - *r,
                BinOp::Mul => (*l as f64) * *r,
                BinOp::Div => (*l as f64) / *r,
                _ => return FieldValue::Null,
            };
            FieldValue::Float(result)
        }
        (FieldValue::Float(l), FieldValue::Integer(r)) => {
            let result = match op {
                BinOp::Add => *l + (*r as f64),
                BinOp::Sub => *l - (*r as f64),
                BinOp::Mul => *l * (*r as f64),
                BinOp::Div => *l / (*r as f64),
                _ => return FieldValue::Null,
            };
            FieldValue::Float(result)
        }
        _ => FieldValue::Null,
    }
}

fn column_index(columns: &[Column], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.name == name)
}

// Class methods for Expr type (tests expect these)
impl Expr {
    /// Parse an expression string into an AST.
    pub fn parse(input: &str, column_names: &[String]) -> Result<Self, ParseError> {
        crate::expr::parser::parse(input, column_names)
    }

    /// Evaluate an expression against a row and return the resulting value.
    pub fn resolve_value(expr: &Expr, row: &[FieldValue], columns: &[Column]) -> FieldValue {
        resolve_value(expr, expr, row, columns)
    }

    /// Evaluate this expression against a single row.
    pub fn evaluate(&self, row: &[FieldValue], columns: &[Column]) -> bool {
        evaluate(self, row, columns)
    }

    /// Collect the names of all columns referenced by this expression,
    /// in first-seen order, without duplicates.
    pub fn referenced_columns(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(self, &mut out);
        out
    }
}

fn walk(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Column(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::Literal(_) => {}
        Expr::BinaryOp(l, _, r) => {
            walk(l, out);
            walk(r, out);
        }
        Expr::LogicalOp(_, l, r) => {
            walk(l, out);
            walk(r, out);
        }
        Expr::Not(inner) => walk(inner, out),
        Expr::Negate(inner) => walk(inner, out),
        Expr::NullCheck(name, _) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::In(name, _) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
}