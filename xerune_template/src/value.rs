//! The model's data as the interpreter sees it: what serde serializes,
//! keeping the Rust types of numbers so they print as Rust prints them.

use std::fmt;

use serde::ser::{self, Serialize};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `()`, and unit structs.
    Unit,
    Bool(bool),
    Int(i128, IntType),
    Float(f64, FloatType),
    Char(char),
    Str(String),
    Option(Option<Box<Value>>),
    /// Sequences: `Vec`s, slices, arrays, sets.
    List(Vec<Value>),
    Tuple(Vec<Value>),
    /// Structs and maps: fields (keys) in order.
    Struct(Vec<(String, Value)>),
    /// An enum variant: its name and payload (`Unit`, a value, a `Tuple`
    /// or a `Struct`).
    Variant(String, Box<Value>),
}

/// The type of an integer; `Untyped` for a literal without a suffix, which
/// takes the type of the other operand, as in Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntType {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    Untyped,
}

/// The type of a float; `Untyped` for a literal without a suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatType {
    F32,
    F64,
    Untyped,
}

impl Value {
    /// What kind of value this is, for messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Unit => "()",
            Value::Bool(_) => "bool",
            Value::Int(..) => "integer",
            Value::Float(..) => "float",
            Value::Char(_) => "char",
            Value::Str(_) => "string",
            Value::Option(_) => "Option",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Struct(_) => "struct",
            Value::Variant(..) => "enum",
        }
    }

    /// The value as `{{ }}` shows it (its `Display`), for the types whose
    /// `Display` is known.
    pub fn display(&self) -> Result<String, String> {
        Ok(match self {
            Value::Bool(b) => b.to_string(),
            Value::Int(i, _) => i.to_string(),
            Value::Float(f, FloatType::F32) => (*f as f32).to_string(),
            Value::Float(f, _) => f.to_string(),
            Value::Char(c) => c.to_string(),
            Value::Str(s) => s.clone(),
            other => return Err(format!("the studio can't show a {} with `{{{{ }}}}`; show a field of it or a string", other.kind())),
        })
    }
}

/// Serializes `value` as a [`Value`].
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, String> {
    value.serialize(ValueSerializer).map_err(|e| e.0)
}

/// Converts a [`Value`] into a method argument.
pub trait FromValue<'a>: Sized {
    fn from_value(value: &'a Value) -> Result<Self, String>;
}

fn mismatch<T>(value: &Value, wanted: &str) -> Result<T, String> {
    Err(format!("expected {wanted}, found {}", value.kind()))
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl FromValue<'_> for $t {
            fn from_value(value: &Value) -> Result<Self, String> {
                match value {
                    Value::Int(i, _) => <$t>::try_from(*i).map_err(|_| format!("{i} doesn't fit in {}", stringify!($t))),
                    other => mismatch(other, stringify!($t)),
                }
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

impl FromValue<'_> for f32 {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Float(f, _) => Ok(*f as f32),
            Value::Int(i, _) => Ok(*i as f32),
            other => mismatch(other, "f32"),
        }
    }
}

impl FromValue<'_> for f64 {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Float(f, _) => Ok(*f),
            Value::Int(i, _) => Ok(*i as f64),
            other => mismatch(other, "f64"),
        }
    }
}

impl FromValue<'_> for bool {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Bool(b) => Ok(*b),
            other => mismatch(other, "bool"),
        }
    }
}

impl FromValue<'_> for char {
    fn from_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Char(c) => Ok(*c),
            other => mismatch(other, "char"),
        }
    }
}

impl<'a> FromValue<'a> for &'a str {
    fn from_value(value: &'a Value) -> Result<Self, String> {
        match value {
            Value::Str(s) => Ok(s),
            other => mismatch(other, "a string"),
        }
    }
}

impl FromValue<'_> for String {
    fn from_value(value: &Value) -> Result<Self, String> {
        <&str>::from_value(value).map(str::to_string)
    }
}

impl<'a> FromValue<'a> for &'a Value {
    fn from_value(value: &'a Value) -> Result<Self, String> {
        Ok(value)
    }
}

impl<'a, T: FromValue<'a>> FromValue<'a> for Option<T> {
    fn from_value(value: &'a Value) -> Result<Self, String> {
        match value {
            Value::Option(None) => Ok(None),
            Value::Option(Some(inner)) => T::from_value(inner).map(Some),
            other => mismatch(other, "an Option"),
        }
    }
}

#[derive(Debug)]
pub struct SerError(String);

impl fmt::Display for SerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SerError {}

impl ser::Error for SerError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerError(msg.to_string())
    }
}

struct ValueSerializer;

/// Collects the elements of a sequence, tuple or tuple variant.
struct SeqBuilder {
    items: Vec<Value>,
    /// `Some(variant)` for a tuple variant; `tuple` for tuples.
    variant: Option<&'static str>,
    tuple: bool,
}

/// Collects the fields of a struct, map or struct variant.
struct StructBuilder {
    fields: Vec<(String, Value)>,
    variant: Option<&'static str>,
    key: Option<String>,
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = SerError;
    type SerializeSeq = SeqBuilder;
    type SerializeTuple = SeqBuilder;
    type SerializeTupleStruct = SeqBuilder;
    type SerializeTupleVariant = SeqBuilder;
    type SerializeMap = StructBuilder;
    type SerializeStruct = StructBuilder;
    type SerializeStructVariant = StructBuilder;

    fn serialize_bool(self, v: bool) -> Result<Value, SerError> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::I8))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::I16))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::I32))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::I64))
    }
    fn serialize_i128(self, v: i128) -> Result<Value, SerError> {
        Ok(Value::Int(v, IntType::I128))
    }
    fn serialize_u8(self, v: u8) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::U8))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::U16))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::U32))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, SerError> {
        Ok(Value::Int(v.into(), IntType::U64))
    }
    fn serialize_u128(self, v: u128) -> Result<Value, SerError> {
        i128::try_from(v).map(|v| Value::Int(v, IntType::U128)).map_err(|_| SerError(format!("{v} is too large")))
    }
    fn serialize_f32(self, v: f32) -> Result<Value, SerError> {
        Ok(Value::Float(v.into(), FloatType::F32))
    }
    fn serialize_f64(self, v: f64) -> Result<Value, SerError> {
        Ok(Value::Float(v, FloatType::F64))
    }
    fn serialize_char(self, v: char) -> Result<Value, SerError> {
        Ok(Value::Char(v))
    }
    fn serialize_str(self, v: &str) -> Result<Value, SerError> {
        Ok(Value::Str(v.to_string()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, SerError> {
        Ok(Value::List(v.iter().map(|b| Value::Int((*b).into(), IntType::U8)).collect()))
    }
    fn serialize_none(self) -> Result<Value, SerError> {
        Ok(Value::Option(None))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, SerError> {
        Ok(Value::Option(Some(Box::new(value.serialize(ValueSerializer)?))))
    }
    fn serialize_unit(self) -> Result<Value, SerError> {
        Ok(Value::Unit)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, SerError> {
        Ok(Value::Unit)
    }
    fn serialize_unit_variant(self, _name: &'static str, _index: u32, variant: &'static str) -> Result<Value, SerError> {
        Ok(Value::Variant(variant.to_string(), Box::new(Value::Unit)))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _name: &'static str, value: &T) -> Result<Value, SerError> {
        value.serialize(ValueSerializer)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, SerError> {
        Ok(Value::Variant(variant.to_string(), Box::new(value.serialize(ValueSerializer)?)))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqBuilder, SerError> {
        Ok(SeqBuilder { items: Vec::with_capacity(len.unwrap_or(0)), variant: None, tuple: false })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqBuilder, SerError> {
        Ok(SeqBuilder { items: Vec::with_capacity(len), variant: None, tuple: true })
    }
    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqBuilder, SerError> {
        self.serialize_tuple(len)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqBuilder, SerError> {
        Ok(SeqBuilder { items: Vec::with_capacity(len), variant: Some(variant), tuple: true })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<StructBuilder, SerError> {
        Ok(StructBuilder { fields: Vec::with_capacity(len.unwrap_or(0)), variant: None, key: None })
    }
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<StructBuilder, SerError> {
        Ok(StructBuilder { fields: Vec::with_capacity(len), variant: None, key: None })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructBuilder, SerError> {
        Ok(StructBuilder { fields: Vec::with_capacity(len), variant: Some(variant), key: None })
    }
}

impl SeqBuilder {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.items.push(value.serialize(ValueSerializer)?);
        Ok(())
    }

    fn finish(self) -> Value {
        let value = if self.tuple { Value::Tuple(self.items) } else { Value::List(self.items) };
        match self.variant {
            Some(variant) => Value::Variant(variant.to_string(), Box::new(value)),
            None => value,
        }
    }
}

impl ser::SerializeSeq for SeqBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTuple for SeqBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleStruct for SeqBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleVariant for SeqBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        self.push(value)
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl StructBuilder {
    fn finish(self) -> Value {
        let value = Value::Struct(self.fields);
        match self.variant {
            Some(variant) => Value::Variant(variant.to_string(), Box::new(value)),
            None => value,
        }
    }
}

impl ser::SerializeMap for StructBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SerError> {
        let key = key.serialize(ValueSerializer)?;
        self.key = Some(match &key {
            Value::Str(s) => s.clone(),
            other => other.display().map_err(SerError)?,
        });
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerError> {
        let key = self.key.take().ok_or_else(|| SerError("map value without a key".to_string()))?;
        self.fields.push((key, value.serialize(ValueSerializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl ser::SerializeStruct for StructBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), SerError> {
        self.fields.push((key.to_string(), value.serialize(ValueSerializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

impl ser::SerializeStructVariant for StructBuilder {
    type Ok = Value;
    type Error = SerError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), SerError> {
        self.fields.push((key.to_string(), value.serialize(ValueSerializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, SerError> {
        Ok(self.finish())
    }
}

/// A [`Value`] read from data that carries no Rust types, as JSON does
/// (`serde_json::from_str::<Value>`), for models that aren't Rust structs:
/// the C API's. `null` is `None` (see the patterns in `render_html`),
/// integers are `i64` (`u64` above its range), other numbers `f64`, and
/// objects are structs with their fields in order.
impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> serde::de::Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v.into(), IntType::I64))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(match i64::try_from(v) {
            Ok(v) => Value::Int(v.into(), IntType::I64),
            Err(_) => Value::Int(v.into(), IntType::U64),
        })
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v, FloatType::F64))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Option(None))
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Option(None))
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        serde::Deserialize::deserialize(deserializer)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut fields: Vec<(String, Value)> = Vec::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            // A repeated key replaces the earlier one, as in JavaScript.
            match fields.iter_mut().find(|(k, _)| *k == key) {
                Some(field) => field.1 = value,
                None => fields.push((key, value)),
            }
        }
        Ok(Value::Struct(fields))
    }
}
