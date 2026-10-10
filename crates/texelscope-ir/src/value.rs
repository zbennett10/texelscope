use std::fmt;

use serde::{Deserialize, Serialize};

/// A value a shader computed, in the shape of its WGSL type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    Bool(bool),
    I32(i32),
    U32(u32),
    F32(#[serde(with = "finite_or_named")] f32),
    Vector(Vec<Value>),
    /// Columns, each a vector.
    Matrix(Vec<Value>),
    Array(Vec<Value>),
    /// Members in declaration order.
    Struct(Vec<(String, Value)>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::I32(v) => write!(f, "{v}i"),
            Self::U32(v) => write!(f, "{v}u"),
            Self::F32(v) => write!(f, "{v}"),
            Self::Vector(items) => write_list(f, &format!("vec{}", items.len()), items),
            Self::Matrix(columns) => {
                let rows = match columns.first() {
                    Some(Self::Vector(column)) => column.len(),
                    _ => 0,
                };
                write_list(f, &format!("mat{}x{rows}", columns.len()), columns)
            }
            Self::Array(items) => write_list(f, "array", items),
            Self::Struct(members) => {
                write!(f, "{{ ")?;
                for (i, (name, value)) in members.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{name}: {value}")?;
                }
                write!(f, " }}")
            }
        }
    }
}

fn write_list(f: &mut fmt::Formatter<'_>, name: &str, items: &[Value]) -> fmt::Result {
    write!(f, "{name}(")?;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{item}")?;
    }
    write!(f, ")")
}

/// JSON has no NaN or infinities, so those travel as the strings JavaScript's `Number()` reads.
mod finite_or_named {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
        if value.is_finite() {
            serializer.serialize_f32(*value)
        } else if value.is_nan() {
            serializer.serialize_str("NaN")
        } else if *value > 0.0 {
            serializer.serialize_str("Infinity")
        } else {
            serializer.serialize_str("-Infinity")
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Number(f32),
            Name(String),
        }
        match Repr::deserialize(deserializer)? {
            Repr::Number(value) => Ok(value),
            Repr::Name(name) => match name.as_str() {
                "NaN" => Ok(f32::NAN),
                "Infinity" => Ok(f32::INFINITY),
                "-Infinity" => Ok(f32::NEG_INFINITY),
                other => Err(D::Error::custom(format!(
                    "expected a number, \"NaN\", \"Infinity\" or \"-Infinity\", found {other:?}"
                ))),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn non_finite_floats_survive_json() {
        for (value, json) in [
            (f32::NAN, r#"{"f32":"NaN"}"#),
            (f32::INFINITY, r#"{"f32":"Infinity"}"#),
            (f32::NEG_INFINITY, r#"{"f32":"-Infinity"}"#),
            (1.5, r#"{"f32":1.5}"#),
        ] {
            assert_eq!(serde_json::to_string(&Value::F32(value)).unwrap(), json);
            let Value::F32(back) = serde_json::from_str(json).unwrap() else {
                panic!("{json} should read back as an f32");
            };
            assert!(back == value || (back.is_nan() && value.is_nan()), "{json}");
        }
    }

    #[test]
    fn displays_like_wgsl() {
        let color = Value::Vector(vec![
            Value::F32(1.0),
            Value::F32(0.25),
            Value::F32(0.0),
            Value::F32(0.8),
        ]);
        assert_eq!(color.to_string(), "vec4(1, 0.25, 0, 0.8)");
        assert_eq!(Value::U32(3).to_string(), "3u");
    }
}
