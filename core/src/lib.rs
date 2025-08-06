use serde::{Deserialize, Serialize};
use std::fmt;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataType {
    Boolean,
    TinyInt,
    SmallInt,
    Integer,
    BigInt,
    Real,
    DoublePrecision,
    Decimal { precision: u8, scale: u8 },
    Char(u16),
    VarChar(Option<u16>),
    Text,
    Binary(u16),
    VarBinary(Option<u16>),
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Interval,
    Uuid,
    Json,
    JsonB,
    Array(Box<DataType>),
}

impl DataType {
    pub fn is_numeric(&self) -> bool {
        matches!(self, 
            DataType::TinyInt | DataType::SmallInt | DataType::Integer | DataType::BigInt |
            DataType::Real | DataType::DoublePrecision | DataType::Decimal { .. }
        )
    }
    
    pub fn is_string(&self) -> bool {
        matches!(self, 
            DataType::Char(_) | DataType::VarChar(_) | DataType::Text
        )
    }
    
    pub fn is_temporal(&self) -> bool {
        matches!(self, 
            DataType::Date | DataType::Time | DataType::Timestamp | 
            DataType::TimestampTz | DataType::Interval
        )
    }
    
    pub fn size_hint(&self) -> Option<usize> {
        match self {
            DataType::Boolean => Some(1),
            DataType::TinyInt => Some(1),
            DataType::SmallInt => Some(2),
            DataType::Integer => Some(4),
            DataType::BigInt => Some(8),
            DataType::Real => Some(4),
            DataType::DoublePrecision => Some(8),
            DataType::Char(n) => Some(*n as usize),
            DataType::Binary(n) => Some(*n as usize),
            DataType::Date => Some(4),
            DataType::Time => Some(8),
            DataType::Timestamp => Some(8),
            DataType::TimestampTz => Some(12),
            DataType::Uuid => Some(16),
            _ => None,
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Boolean => write!(f, "BOOLEAN"),
            DataType::TinyInt => write!(f, "TINYINT"),
            DataType::SmallInt => write!(f, "SMALLINT"),
            DataType::Integer => write!(f, "INTEGER"),
            DataType::BigInt => write!(f, "BIGINT"),
            DataType::Real => write!(f, "REAL"),
            DataType::DoublePrecision => write!(f, "DOUBLE PRECISION"),
            DataType::Decimal { precision, scale } => write!(f, "DECIMAL({}, {})", precision, scale),
            DataType::Char(n) => write!(f, "CHAR({})", n),
            DataType::VarChar(Some(n)) => write!(f, "VARCHAR({})", n),
            DataType::VarChar(None) => write!(f, "VARCHAR"),
            DataType::Text => write!(f, "TEXT"),
            DataType::Binary(n) => write!(f, "BINARY({})", n),
            DataType::VarBinary(Some(n)) => write!(f, "VARBINARY({})", n),
            DataType::VarBinary(None) => write!(f, "VARBINARY"),
            DataType::Date => write!(f, "DATE"),
            DataType::Time => write!(f, "TIME"),
            DataType::Timestamp => write!(f, "TIMESTAMP"),
            DataType::TimestampTz => write!(f, "TIMESTAMP WITH TIME ZONE"),
            DataType::Interval => write!(f, "INTERVAL"),
            DataType::Uuid => write!(f, "UUID"),
            DataType::Json => write!(f, "JSON"),
            DataType::JsonB => write!(f, "JSONB"),
            DataType::Array(inner) => write!(f, "{}[]", inner),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Boolean(bool),
    TinyInt(i8),
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Real(f32),
    DoublePrecision(f64),
    Decimal(rust_decimal::Decimal),
    Char(String),
    VarChar(String),
    Text(String),
    Binary(Vec<u8>),
    VarBinary(Vec<u8>),
    Date(chrono::NaiveDate),
    Time(chrono::NaiveTime),
    Timestamp(chrono::NaiveDateTime),
    TimestampTz(chrono::DateTime<chrono::Utc>),
    Interval(std::time::Duration),
    Uuid(uuid::Uuid),
    Json(serde_json::Value),
    JsonB(serde_json::Value),
    Array(Vec<Value>),
}

impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Null => panic!("Cannot determine type of NULL value"),
            Value::Boolean(_) => DataType::Boolean,
            Value::TinyInt(_) => DataType::TinyInt,
            Value::SmallInt(_) => DataType::SmallInt,
            Value::Integer(_) => DataType::Integer,
            Value::BigInt(_) => DataType::BigInt,
            Value::Real(_) => DataType::Real,
            Value::DoublePrecision(_) => DataType::DoublePrecision,
            Value::Decimal(_) => DataType::Decimal { precision: 28, scale: 10 },
            Value::Char(s) => DataType::Char(s.len() as u16),
            Value::VarChar(_) => DataType::VarChar(None),
            Value::Text(_) => DataType::Text,
            Value::Binary(b) => DataType::Binary(b.len() as u16),
            Value::VarBinary(_) => DataType::VarBinary(None),
            Value::Date(_) => DataType::Date,
            Value::Time(_) => DataType::Time,
            Value::Timestamp(_) => DataType::Timestamp,
            Value::TimestampTz(_) => DataType::TimestampTz,
            Value::Interval(_) => DataType::Interval,
            Value::Uuid(_) => DataType::Uuid,
            Value::Json(_) => DataType::Json,
            Value::JsonB(_) => DataType::JsonB,
            Value::Array(arr) => {
                if let Some(first) = arr.first() {
                    DataType::Array(Box::new(first.data_type()))
                } else {
                    panic!("Cannot determine type of empty array")
                }
            }
        }
    }
    
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    
    pub fn type_compatible(&self, data_type: &DataType) -> bool {
        if self.is_null() {
            return true;
        }
        
        match (self, data_type) {
            (Value::Boolean(_), DataType::Boolean) => true,
            (Value::TinyInt(_), DataType::TinyInt) => true,
            (Value::SmallInt(_), DataType::SmallInt) => true,
            (Value::Integer(_), DataType::Integer) => true,
            (Value::BigInt(_), DataType::BigInt) => true,
            (Value::Real(_), DataType::Real) => true,
            (Value::DoublePrecision(_), DataType::DoublePrecision) => true,
            (Value::Decimal(_), DataType::Decimal { .. }) => true,
            (Value::Char(_), DataType::Char(_)) => true,
            (Value::VarChar(_), DataType::VarChar(_)) => true,
            (Value::Text(_), DataType::Text) => true,
            (Value::Binary(_), DataType::Binary(_)) => true,
            (Value::VarBinary(_), DataType::VarBinary(_)) => true,
            (Value::Date(_), DataType::Date) => true,
            (Value::Time(_), DataType::Time) => true,
            (Value::Timestamp(_), DataType::Timestamp) => true,
            (Value::TimestampTz(_), DataType::TimestampTz) => true,
            (Value::Interval(_), DataType::Interval) => true,
            (Value::Uuid(_), DataType::Uuid) => true,
            (Value::Json(_), DataType::Json) => true,
            (Value::JsonB(_), DataType::JsonB) => true,
            (Value::Array(_), DataType::Array(_)) => true,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::TinyInt(i) => write!(f, "{}", i),
            Value::SmallInt(i) => write!(f, "{}", i),
            Value::Integer(i) => write!(f, "{}", i),
            Value::BigInt(i) => write!(f, "{}", i),
            Value::Real(r) => write!(f, "{}", r),
            Value::DoublePrecision(d) => write!(f, "{}", d),
            Value::Decimal(d) => write!(f, "{}", d),
            Value::Char(s) | Value::VarChar(s) | Value::Text(s) => write!(f, "'{}'", s),
            Value::Binary(b) | Value::VarBinary(b) => write!(f, "0x{}", hex::encode(b)),
            Value::Date(d) => write!(f, "'{}'", d.format("%Y-%m-%d")),
            Value::Time(t) => write!(f, "'{}'", t.format("%H:%M:%S")),
            Value::Timestamp(ts) => write!(f, "'{}'", ts.format("%Y-%m-%d %H:%M:%S")),
            Value::TimestampTz(ts) => write!(f, "'{}'", ts.format("%Y-%m-%d %H:%M:%S %Z")),
            Value::Interval(dur) => write!(f, "'{} seconds'", dur.as_secs()),
            Value::Uuid(u) => write!(f, "'{}'", u),
            Value::Json(j) | Value::JsonB(j) => write!(f, "'{}'", j),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, val) in arr.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", val)?;
                }
                write!(f, "]")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub default_value: Option<Value>,
    pub primary_key: bool,
    pub unique: bool,
    pub auto_increment: bool,
    pub check_constraint: Option<String>,
}

impl ColumnDefinition {
    pub fn new(name: String, data_type: DataType) -> Self {
        Self {
            name,
            data_type,
            nullable: true,
            default_value: None,
            primary_key: false,
            unique: false,
            auto_increment: false,
            check_constraint: None,
        }
    }
    
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }
    
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }
    
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
    
    pub fn default(mut self, value: Value) -> Self {
        self.default_value = Some(value);
        self
    }
    
    pub fn auto_increment(mut self) -> Self {
        self.auto_increment = true;
        self
    }
    
    pub fn check(mut self, constraint: String) -> Self {
        self.check_constraint = Some(constraint);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDefinition>,
    pub indexes: Vec<IndexDefinition>,
    pub foreign_keys: Vec<ForeignKeyDefinition>,
}

impl TableSchema {
    pub fn new(name: String) -> Self {
        Self {
            name,
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        }
    }
    
    pub fn add_column(mut self, column: ColumnDefinition) -> Self {
        self.columns.push(column);
        self
    }
    
    pub fn add_index(mut self, index: IndexDefinition) -> Self {
        self.indexes.push(index);
        self
    }
    
    pub fn add_foreign_key(mut self, fk: ForeignKeyDefinition) -> Self {
        self.foreign_keys.push(fk);
        self
    }
    
    pub fn get_column(&self, name: &str) -> Option<&ColumnDefinition> {
        self.columns.iter().find(|col| col.name == name)
    }
    
    pub fn get_primary_key_columns(&self) -> Vec<&ColumnDefinition> {
        self.columns.iter().filter(|col| col.primary_key).collect()
    }
    
    pub fn validate_row(&self, values: &HashMap<String, Value>) -> Result<(), String> {
        for column in &self.columns {
            if let Some(value) = values.get(&column.name) {
                if !value.type_compatible(&column.data_type) {
                    return Err(format!(
                        "Type mismatch for column '{}': expected {}, got {}",
                        column.name, column.data_type, value.data_type()
                    ));
                }
            } else if !column.nullable && column.default_value.is_none() {
                return Err(format!("Column '{}' cannot be null", column.name));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub index_type: IndexType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IndexType {
    BTree,
    Hash,
    GiST,
    GIN,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeyDefinition {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_delete: ReferentialAction,
    pub on_update: ReferentialAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReferentialAction {
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}
