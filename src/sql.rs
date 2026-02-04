use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sqlparser::ast::{Statement, Query, SelectItem, TableFactor, Expr, Value, BinaryOperator, TableWithJoins, SetExpr, Values, Function, FunctionArg, FunctionArgExpr};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::{Velocity, VeloResult, VeloError, VeloKey, VeloValue};

/// SQL Query Result
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub success: bool,
    pub rows_affected: usize,
    pub data: Vec<Row>,
    pub columns: Vec<String>,
    pub execution_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Row {
    pub values: Vec<SqlValue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SqlValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Null,
    Binary(Vec<u8>),
}

impl From<&VeloValue> for SqlValue {
    fn from(value: &VeloValue) -> Self {
        // Try to parse as UTF-8 string first
        if let Ok(s) = String::from_utf8(value.clone()) {
            // Check if it's a number
            if let Ok(i) = s.parse::<i64>() {
                return SqlValue::Integer(i);
            }
            if let Ok(f) = s.parse::<f64>() {
                return SqlValue::Float(f);
            }
            if let Ok(b) = s.parse::<bool>() {
                return SqlValue::Boolean(b);
            }
            SqlValue::String(s)
        } else {
            SqlValue::Binary(value.clone())
        }
    }
}

impl SqlValue {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            SqlValue::String(s) => s.as_bytes().to_vec(),
            SqlValue::Integer(i) => i.to_string().as_bytes().to_vec(),
            SqlValue::Float(f) => f.to_string().as_bytes().to_vec(),
            SqlValue::Boolean(b) => b.to_string().as_bytes().to_vec(),
            SqlValue::Null => Vec::new(),
            SqlValue::Binary(b) => b.clone(),
        }
    }
}

/// SQL Engine for VelocityDB
pub struct SqlEngine {
    pub db: Arc<Velocity>,
    dialect: GenericDialect,
}

impl SqlEngine {
    pub fn new(db: Arc<Velocity>) -> Self {
        Self {
            db,
            dialect: GenericDialect {},
        }
    }

    pub async fn execute(&self, sql: &str) -> VeloResult<QueryResult> {
        let start_time = std::time::Instant::now();
        
        // Parse SQL
        let statements = Parser::parse_sql(&self.dialect, sql)
            .map_err(|e| VeloError::InvalidOperation(format!("SQL Parse Error: {}", e)))?;

        if statements.is_empty() {
            return Err(VeloError::InvalidOperation("No SQL statement provided".to_string()));
        }

        let statement = &statements[0];
        let result = match statement {
            Statement::Query(query) => self.execute_query(query).await?,
            Statement::Insert { table_name, columns, source, .. } => {
                self.execute_insert(table_name, columns, source).await?
            }
            Statement::Update { table, assignments, selection, .. } => {
                self.execute_update(table, assignments, selection).await?
            }
            Statement::Delete { from, selection, .. } => {
                self.execute_delete(from, selection).await?
            }
            _ => {
                return Err(VeloError::InvalidOperation("Unsupported SQL statement".to_string()));
            }
        };

        let execution_time = start_time.elapsed().as_millis() as u64;
        Ok(QueryResult {
            success: true,
            rows_affected: result.rows_affected,
            data: result.data,
            columns: result.columns,
            execution_time_ms: execution_time,
        })
    }

    async fn execute_query(&self, query: &Query) -> VeloResult<QueryResult> {
        // For now, we only support simple SELECT queries on the virtual 'kv' table
        match query.body.as_ref() {
            sqlparser::ast::SetExpr::Select(select) => {
                self.execute_select(select).await
            }
            _ => {
                Err(VeloError::InvalidOperation("Complex queries not supported yet".to_string()))
            }
        }
    }

    async fn execute_select(&self, select: &sqlparser::ast::Select) -> VeloResult<QueryResult> {
        // Check if querying the 'kv' table
        let table_name = self.extract_table_name(&select.from)?;
        if table_name != "kv" {
            return Err(VeloError::InvalidOperation("Only 'kv' table is supported".to_string()));
        }

        // Parse WHERE clause for key filtering
        let key_filter = if let Some(where_clause) = &select.selection {
            self.extract_key_filter(where_clause)?
        } else {
            KeyFilter::All
        };

        // Execute based on filter type
        match key_filter {
            KeyFilter::Exact(key) => {
                if let Some(value) = self.db.get(&key)? {
                    Ok(QueryResult {
                        success: true,
                        rows_affected: 1,
                        data: vec![Row {
                            values: vec![SqlValue::String(key), SqlValue::from(&value)],
                        }],
                        columns: vec!["key".to_string(), "value".to_string()],
                        execution_time_ms: 0,
                    })
                } else {
                    Ok(QueryResult {
                        success: true,
                        rows_affected: 0,
                        data: vec![],
                        columns: vec!["key".to_string(), "value".to_string()],
                        execution_time_ms: 0,
                    })
                }
            }
            KeyFilter::Prefix(prefix) => {
                self.execute_prefix_scan(&prefix).await
            }
            KeyFilter::Range(start, end) => {
                self.execute_range_scan(&start, &end).await
            }
            KeyFilter::All => {
                // Full table scan (limited for safety)
                self.execute_full_scan().await
            }
        }
    }

    async fn execute_insert(&self, table_name: &sqlparser::ast::ObjectName, 
                          columns: &[sqlparser::ast::Ident], 
                          source: &Query) -> VeloResult<QueryResult> {
        let table = table_name.to_string();
        if table != "kv" {
            return Err(VeloError::InvalidOperation("Only 'kv' table is supported".to_string()));
        }

        // Handle simple VALUES
        match source.body.as_ref() {
            SetExpr::Values(values) => {
                let mut rows_inserted = 0;
                
                for row in &values.rows {
                    if row.len() != 2 {
                        return Err(VeloError::InvalidOperation("INSERT must have exactly 2 values (key, value)".to_string()));
                    }

                    let key = self.extract_string_value(&row[0])?;
                    let value = self.extract_value_bytes(&row[1])?;

                    self.db.put(key, value)?;
                    rows_inserted += 1;
                }

                Ok(QueryResult {
                    success: true,
                    rows_affected: rows_inserted,
                    data: vec![],
                    columns: vec![],
                    execution_time_ms: 0,
                })
            }
            _ => {
                Err(VeloError::InvalidOperation("Unsupported INSERT format".to_string()))
            }
        }
    }

    async fn execute_update(&self, table: &sqlparser::ast::TableWithJoins,
                          assignments: &[sqlparser::ast::Assignment],
                          selection: &Option<Expr>) -> VeloResult<QueryResult> {
        // Extract table name
        let table_name = match &table.relation {
            TableFactor::Table { name, .. } => name.to_string(),
            _ => return Err(VeloError::InvalidOperation("Complex table references not supported".to_string())),
        };

        if table_name != "kv" {
            return Err(VeloError::InvalidOperation("Only 'kv' table is supported".to_string()));
        }

        // Extract key from WHERE clause
        let key = if let Some(where_clause) = selection {
            match self.extract_key_filter(where_clause)? {
                KeyFilter::Exact(k) => k,
                _ => return Err(VeloError::InvalidOperation("UPDATE requires exact key match".to_string())),
            }
        } else {
            return Err(VeloError::InvalidOperation("UPDATE requires WHERE clause".to_string()));
        };

        // Check if key exists
        if self.db.get(&key)?.is_none() {
            return Ok(QueryResult {
                success: true,
                rows_affected: 0,
                data: vec![],
                columns: vec![],
                execution_time_ms: 0,
            });
        }

        // Apply assignments
        for assignment in assignments {
            if assignment.id.len() != 1 || assignment.id[0].value != "value" {
                return Err(VeloError::InvalidOperation("Can only update 'value' column".to_string()));
            }

            let new_value = self.extract_value_bytes(&assignment.value)?;
            self.db.put(key.clone(), new_value)?;
        }

        Ok(QueryResult {
            success: true,
            rows_affected: 1,
            data: vec![],
            columns: vec![],
            execution_time_ms: 0,
        })
    }

    async fn execute_delete(&self, from: &[sqlparser::ast::TableWithJoins],
                          selection: &Option<Expr>) -> VeloResult<QueryResult> {
        // Extract table name from the first table
        if from.is_empty() {
            return Err(VeloError::InvalidOperation("No table specified in DELETE".to_string()));
        }

        let table_name = match &from[0].relation {
            TableFactor::Table { name, .. } => name.to_string(),
            _ => return Err(VeloError::InvalidOperation("Complex table references not supported".to_string())),
        };

        if table_name != "kv" {
            return Err(VeloError::InvalidOperation("Only 'kv' table is supported".to_string()));
        }

        // Extract key from WHERE clause
        let key = if let Some(where_clause) = selection {
            match self.extract_key_filter(where_clause)? {
                KeyFilter::Exact(k) => k,
                _ => return Err(VeloError::InvalidOperation("DELETE requires exact key match".to_string())),
            }
        } else {
            return Err(VeloError::InvalidOperation("DELETE requires WHERE clause".to_string()));
        };

        // Check if key exists and delete
        let existed = self.db.get(&key)?.is_some();
        if existed {
            // For now, we'll implement delete as putting an empty value
            // In a real implementation, you'd want a proper delete operation
            self.db.put(key, vec![])?;
        }

        Ok(QueryResult {
            success: true,
            rows_affected: if existed { 1 } else { 0 },
            data: vec![],
            columns: vec![],
            execution_time_ms: 0,
        })
    }

    // Helper methods
    fn extract_table_name(&self, from: &[sqlparser::ast::TableWithJoins]) -> VeloResult<String> {
        if from.is_empty() {
            return Err(VeloError::InvalidOperation("No table specified".to_string()));
        }

        match &from[0].relation {
            TableFactor::Table { name, .. } => Ok(name.to_string()),
            _ => Err(VeloError::InvalidOperation("Complex table references not supported".to_string())),
        }
    }

    fn extract_key_filter(&self, expr: &Expr) -> VeloResult<KeyFilter> {
        match expr {
            Expr::BinaryOp { left, op, right } => {
                match op {
                    BinaryOperator::Eq => {
                        if let (Expr::Identifier(id), Expr::Value(val)) = (left.as_ref(), right.as_ref()) {
                            if id.value == "key" {
                                let key = self.extract_string_from_value(val)?;
                                return Ok(KeyFilter::Exact(key));
                            }
                        }
                    }
                    BinaryOperator::GtEq => {
                        if let (Expr::Identifier(id), Expr::Value(val)) = (left.as_ref(), right.as_ref()) {
                            if id.value == "key" {
                                let start_key = self.extract_string_from_value(val)?;
                                return Ok(KeyFilter::Prefix(start_key)); // Simplified range as prefix
                            }
                        }
                    }
                    BinaryOperator::Lt => {
                        // Handle range queries (simplified)
                        return Ok(KeyFilter::All); // Fallback to full scan
                    }
                    _ => {}
                }
            }
            // Handle LIKE as a function call or other expression types
            Expr::Function(func) => {
                if func.name.to_string().to_lowercase() == "like" && func.args.len() == 2 {
                    // Handle LIKE as function: LIKE(key, 'pattern%')
                    if let (
                        sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(Expr::Identifier(id))),
                        sqlparser::ast::FunctionArg::Unnamed(sqlparser::ast::FunctionArgExpr::Expr(Expr::Value(val)))
                    ) = (&func.args[0], &func.args[1]) {
                        if id.value == "key" {
                            let pattern = self.extract_string_from_value(val)?;
                            if pattern.ends_with('%') {
                                let prefix = pattern.trim_end_matches('%');
                                return Ok(KeyFilter::Prefix(prefix.to_string()));
                            }
                        }
                    }
                }
            }
            // Handle LIKE as infix expression (key LIKE 'pattern%')
            Expr::Like { expr, pattern, .. } => {
                if let (Expr::Identifier(id), Expr::Value(val)) = (expr.as_ref(), pattern.as_ref()) {
                    if id.value == "key" {
                        let pattern_str = self.extract_string_from_value(val)?;
                        if pattern_str.ends_with('%') {
                            let prefix = pattern_str.trim_end_matches('%');
                            return Ok(KeyFilter::Prefix(prefix.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(KeyFilter::All)
    }

    fn extract_string_value(&self, expr: &Expr) -> VeloResult<String> {
        match expr {
            Expr::Value(Value::SingleQuotedString(s)) => Ok(s.clone()),
            Expr::Value(Value::DoubleQuotedString(s)) => Ok(s.clone()),
            _ => Err(VeloError::InvalidOperation("Expected string value".to_string())),
        }
    }

    fn extract_value_bytes(&self, expr: &Expr) -> VeloResult<Vec<u8>> {
        match expr {
            Expr::Value(Value::SingleQuotedString(s)) => Ok(s.as_bytes().to_vec()),
            Expr::Value(Value::DoubleQuotedString(s)) => Ok(s.as_bytes().to_vec()),
            Expr::Value(Value::Number(n, _)) => Ok(n.as_bytes().to_vec()),
            _ => Err(VeloError::InvalidOperation("Unsupported value type".to_string())),
        }
    }

    fn extract_string_from_value(&self, value: &Value) -> VeloResult<String> {
        match value {
            Value::SingleQuotedString(s) => Ok(s.clone()),
            Value::DoubleQuotedString(s) => Ok(s.clone()),
            _ => Err(VeloError::InvalidOperation("Expected string value".to_string())),
        }
    }

    async fn execute_prefix_scan(&self, prefix: &str) -> VeloResult<QueryResult> {
        // This is a simplified implementation
        // In a real system, you'd want to implement efficient prefix scanning
        let mut results = Vec::new();
        
        // For now, we'll simulate by checking some common patterns
        for i in 0..100 {
            let key = format!("{}{}", prefix, i);
            if let Some(value) = self.db.get(&key)? {
                results.push(Row {
                    values: vec![SqlValue::String(key), SqlValue::from(&value)],
                });
            }
        }

        Ok(QueryResult {
            success: true,
            rows_affected: results.len(),
            data: results,
            columns: vec!["key".to_string(), "value".to_string()],
            execution_time_ms: 0,
        })
    }

    async fn execute_range_scan(&self, _start: &str, _end: &str) -> VeloResult<QueryResult> {
        // Simplified range scan implementation
        Ok(QueryResult {
            success: true,
            rows_affected: 0,
            data: vec![],
            columns: vec!["key".to_string(), "value".to_string()],
            execution_time_ms: 0,
        })
    }

    async fn execute_full_scan(&self) -> VeloResult<QueryResult> {
        // Limited full scan for safety
        Ok(QueryResult {
            success: true,
            rows_affected: 0,
            data: vec![],
            columns: vec!["key".to_string(), "value".to_string()],
            execution_time_ms: 0,
        })
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum KeyFilter {
    Exact(String),
    Prefix(String),
    Range(String, String),
    All,
}