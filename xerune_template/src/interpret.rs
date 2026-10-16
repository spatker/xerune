//! Runs a [`Template`] against a model's data and writes the view as HTML,
//! for the studio: template edits show without recompiling.
//!
//! It follows the code `#[derive(XeruneTemplate)]` generates: the same
//! scoping (bare names are model fields unless a `for` or `if let` binds
//! them), conditions must be `bool`s, numbers keep their Rust types, and
//! `{{ }}` prints a value as its `Display` does. The model's methods come
//! from a table the derive generates ([`Methods`]); every element carries a
//! key in `data-xk` so the page can update itself in place.

use std::borrow::Cow;

use crate::expr::{BinOp, Cond, Expr, Pattern, RangeOp, UnaryOp};
use crate::format::format;
use crate::source::Error;
use crate::tree::{Attr, AttrPart, Kind, NodeId, Template};
use crate::value::{FloatType, IntType, Value};

/// The model's methods a template may call: the calls the template made
/// when the studio was built, by [`Expr::method_key`].
pub trait Methods {
    /// The result of method `key` with `args`, `None` if there is no such
    /// method in the table.
    fn call(&self, key: &str, args: &[Value]) -> Option<Result<Value, String>>;
}

/// No methods.
impl Methods for () {
    fn call(&self, _key: &str, _args: &[Value]) -> Option<Result<Value, String>> {
        None
    }
}

/// The view of `template` for a model whose data is `model`, as HTML: the
/// `<body>` and its content. `<head>` and `<style>` are left out (see
/// [`Template::css`]).
pub fn render_html(template: &Template, model: &Value, methods: &dyn Methods) -> Result<String, Error> {
    let mut interp = Interp { tpl: template, model, methods, scopes: Vec::new(), loops: Vec::new(), out: String::new() };
    interp.children(Template::ROOT, Keys { parent: 0, loop_key: 0 }).map_err(Error)?;
    Ok(interp.out)
}

/// What a `{% break %}` / `{% continue %}` does to the enclosing loop.
#[derive(PartialEq)]
enum Flow {
    Normal,
    Break,
    Continue,
}

/// The keys a node's key derives from, as in the compiled template.
#[derive(Clone, Copy)]
struct Keys {
    parent: u64,
    /// The key of the enclosing loop iteration's scope, for `data-key`s.
    loop_key: u64,
}

fn mix(key: u64, site: u64) -> u64 {
    let mut hash = key ^ 0xcbf2_9ce4_8422_2325;
    for byte in site.to_le_bytes() {
        hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn hash_str(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            c => out.push(c),
        }
    }
}

struct Interp<'a> {
    tpl: &'a Template,
    model: &'a Value,
    methods: &'a dyn Methods,
    /// Bindings of `for` / `if let`, innermost last.
    scopes: Vec<(String, Value)>,
    /// The index of each enclosing loop's current iteration.
    loops: Vec<usize>,
    out: String,
}

type R<T> = Result<T, String>;

impl<'a> Interp<'a> {
    fn children(&mut self, id: NodeId, keys: Keys) -> R<Flow> {
        for &child in self.tpl.children(id) {
            let flow = self.node(child, keys)?;
            if flow != Flow::Normal {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn node(&mut self, id: NodeId, keys: Keys) -> R<Flow> {
        match &self.tpl.node(id).kind {
            Kind::Document | Kind::Branch(_) => self.children(id, keys),
            Kind::Text(text) => {
                escape_text(text, &mut self.out);
                Ok(Flow::Normal)
            }
            Kind::Expr(expr) => {
                let text = self.eval(expr).and_then(|v| v.display()).map_err(|e| format!("{e}\n  in `{{{{ {expr} }}}}`"))?;
                escape_text(&text, &mut self.out);
                Ok(Flow::Normal)
            }
            Kind::Break => Ok(Flow::Break),
            Kind::Continue => Ok(Flow::Continue),
            Kind::If => {
                for &branch in self.tpl.children(id) {
                    let Kind::Branch(cond) = &self.tpl.node(branch).kind else { continue };
                    let depth = self.scopes.len();
                    let taken = match cond {
                        None => true,
                        Some(cond) => self.test(cond).map_err(|e| format!("{e}\n  in `{{% if {cond} %}}`"))?,
                    };
                    if taken {
                        let flow = self.children(branch, keys);
                        self.scopes.truncate(depth);
                        return flow;
                    }
                    // A pattern that failed halfway may have bound names.
                    self.scopes.truncate(depth);
                }
                Ok(Flow::Normal)
            }
            Kind::For(pattern, iter) => {
                let context = |e: String| format!("{e}\n  in `{{% for {pattern} in {iter} %}}`");
                let items = self.iterate(iter).map_err(context)?;
                let loop_key = mix(keys.parent, id as u64);
                let depth = self.scopes.len();
                for (index, item) in items.into_iter().enumerate() {
                    if !bind(pattern, item, &mut self.scopes) {
                        return Err(context(format!("an item doesn't match `{pattern}`")));
                    }
                    self.loops.push(index);
                    let flow = self.children(id, Keys { parent: mix(loop_key, index as u64), loop_key })?;
                    self.loops.pop();
                    self.scopes.truncate(depth);
                    if flow == Flow::Break {
                        break;
                    }
                }
                Ok(Flow::Normal)
            }
            Kind::Element { tag, attrs } => {
                match tag.as_str() {
                    "head" | "style" | "script" | "template" => return Ok(Flow::Normal),
                    // The page has its own; the body's attributes and
                    // content go to the page's body.
                    "html" => return self.children(id, keys),
                    _ => {}
                }
                let key = match attrs.iter().find(|a| a.name == "data-key") {
                    Some(attr) => {
                        let text = self.attr_text(attr)?;
                        mix(mix(keys.loop_key, hash_str(&text)), id as u64)
                    }
                    None => mix(keys.parent, id as u64),
                };
                self.out.push('<');
                self.out.push_str(tag);
                for attr in attrs {
                    let value = if attr.is_dynamic() { Cow::Owned(self.attr_text(attr)?) } else { Cow::Borrowed(attr.value.as_str()) };
                    // `checked="false"` is unchecked in xerune; in HTML
                    // any `checked` is checked.
                    if attr.name == "checked" && value == "false" {
                        continue;
                    }
                    self.out.push(' ');
                    self.out.push_str(&attr.name);
                    self.out.push_str("=\"");
                    escape_attr(&value, &mut self.out);
                    self.out.push('"');
                }
                self.out.push_str(&format!(" data-xk=\"{key:x}\">"));
                if VOID.contains(&tag.as_str()) {
                    return Ok(Flow::Normal);
                }
                let flow = self.children(id, Keys { parent: key, loop_key: key })?;
                self.out.push_str("</");
                self.out.push_str(tag);
                self.out.push('>');
                Ok(flow)
            }
        }
    }

    fn attr_text(&mut self, attr: &Attr) -> R<String> {
        let mut out = String::new();
        match &attr.parts {
            Some(parts) => self.attr_parts(parts, &mut out).map_err(|e| format!("{e}\n  in `{}=\"{}\"`", attr.name, attr.value))?,
            None => out.push_str(&attr.value),
        }
        Ok(out)
    }

    fn attr_parts(&mut self, parts: &[AttrPart], out: &mut String) -> R<()> {
        for part in parts {
            match part {
                AttrPart::Lit(text) => out.push_str(text),
                AttrPart::Expr(expr) => out.push_str(&self.eval(expr)?.display()?),
                AttrPart::If(branches) => {
                    for branch in branches {
                        let depth = self.scopes.len();
                        let taken = match &branch.cond {
                            None => true,
                            Some(cond) => self.test(cond)?,
                        };
                        if taken {
                            let result = self.attr_parts(&branch.parts, out);
                            self.scopes.truncate(depth);
                            result?;
                            break;
                        }
                        self.scopes.truncate(depth);
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether `cond` holds; `if let` binds its names (the caller drops
    /// them after the branch).
    fn test(&mut self, cond: &Cond) -> R<bool> {
        let value = self.eval(&cond.expr)?.into_owned();
        match &cond.pattern {
            Some(pattern) => Ok(bind(pattern, value, &mut self.scopes)),
            None => as_bool(&value),
        }
    }

    fn iterate(&self, iter: &Expr) -> R<Vec<Value>> {
        Ok(match self.eval(iter)?.into_owned() {
            Value::List(items) | Value::Tuple(items) => items,
            Value::Struct(fields) => fields.into_iter().map(|(k, v)| Value::Tuple(vec![Value::Str(k), v])).collect(),
            Value::Option(item) => item.into_iter().map(|b| *b).collect(),
            other => return Err(format!("can't loop over a {}", other.kind())),
        })
    }

    fn is_local(&self, name: &str) -> bool {
        (name == "loop" && !self.loops.is_empty()) || self.scopes.iter().any(|(n, _)| n == name)
    }

    fn eval(&self, expr: &Expr) -> R<Cow<'_, Value>> {
        Ok(match expr {
            Expr::Bool(b) => Cow::Owned(Value::Bool(*b)),
            Expr::Num(n) => Cow::Owned(number(n)?),
            Expr::Str(s) => Cow::Owned(Value::Str(unescape(s)?)),
            Expr::Char(c) => {
                let s = unescape(c)?;
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Cow::Owned(Value::Char(c)),
                    _ => return Err(format!("'{c}' is not one character")),
                }
            }
            Expr::Var(name) => {
                if let Some((_, value)) = self.scopes.iter().rev().find(|(n, _)| n == name) {
                    Cow::Borrowed(value)
                } else if name == "self" {
                    Cow::Borrowed(self.model)
                } else {
                    Cow::Borrowed(field(self.model, name).map_err(|_| format!("the model has no field `{name}`"))?)
                }
            }
            Expr::Path(parts) => Cow::Owned(constant(parts.last().map_or("", String::as_str), None)),
            Expr::Array(items) => Cow::Owned(Value::List(items.iter().map(|e| self.eval(e).map(Cow::into_owned)).collect::<R<_>>()?)),
            Expr::Tuple(items) => Cow::Owned(Value::Tuple(items.iter().map(|e| self.eval(e).map(Cow::into_owned)).collect::<R<_>>()?)),
            Expr::Attr(obj, name) => {
                if matches!(&**obj, Expr::Var(v) if v == "loop") && self.is_local("loop") && !self.scopes.iter().any(|(n, _)| n == "loop") {
                    let index = *self.loops.last().unwrap_or(&0) as i128;
                    return Ok(Cow::Owned(Value::Int(if name == "index" { index + 1 } else { index }, IntType::U64)));
                }
                match self.eval(obj)? {
                    Cow::Borrowed(value) => Cow::Borrowed(field(value, name)?),
                    Cow::Owned(value) => Cow::Owned(field(&value, name)?.clone()),
                }
            }
            Expr::Index(obj, index) => {
                let index = self.eval(index)?.into_owned();
                let element = |value: &Value| -> R<Value> {
                    match (value, &index) {
                        (Value::List(items), Value::Int(i, _)) => {
                            items.get(*i as usize).cloned().ok_or_else(|| format!("index {i} is out of bounds (length {})", items.len()))
                        }
                        (Value::Struct(_), Value::Str(key)) => field(value, key).cloned(),
                        (value, index) => Err(format!("can't index a {} with a {}", value.kind(), index.kind())),
                    }
                };
                Cow::Owned(element(&*self.eval(obj)?)?)
            }
            Expr::Unary(op, inner) => {
                let value = self.eval(inner)?;
                Cow::Owned(match (op, &*value) {
                    (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
                    (UnaryOp::Not, Value::Int(i, ty)) => Value::Int(!i, *ty),
                    (UnaryOp::Neg, Value::Int(i, ty)) => Value::Int(-i, *ty),
                    (UnaryOp::Neg, Value::Float(f, ty)) => Value::Float(-f, *ty),
                    (UnaryOp::Deref | UnaryOp::Ref, value) => value.clone(),
                    (op, value) => return Err(format!("can't apply `{}` to a {}", op.as_str(), value.kind())),
                })
            }
            Expr::Binary(op, left, right) => Cow::Owned(self.binary(*op, left, right)?),
            Expr::Range(op, start, end) => {
                let bound = |e: &Option<Box<Expr>>| -> R<(i128, IntType)> {
                    match e.as_deref().map(|e| self.eval(e)).transpose()?.as_deref() {
                        Some(Value::Int(i, ty)) => Ok((*i, *ty)),
                        Some(other) => Err(format!("a range of {}s", other.kind())),
                        None => Err("ranges need both bounds in templates".to_string()),
                    }
                };
                let ((start, ta), (end, tb)) = (bound(start)?, bound(end)?);
                let ty = if ta == IntType::Untyped { tb } else { ta };
                let end = if *op == RangeOp::Inclusive { end + 1 } else { end };
                Cow::Owned(Value::List((start..end).map(|i| Value::Int(i, ty)).collect()))
            }
            Expr::Group(inner) => self.eval(inner)?,
            Expr::Call(callee, args) => Cow::Owned(self.call(callee, args)?),
            Expr::Format(args) => {
                let Expr::Str(fmt) = &args[0] else {
                    return Err("the format string must be a string literal".to_string());
                };
                let values = args[1..].iter().map(|e| self.eval(e).map(Cow::into_owned)).collect::<R<Vec<_>>>()?;
                Cow::Owned(Value::Str(format(&unescape(fmt)?, &values)?))
            }
        })
    }

    fn call(&self, callee: &Expr, args: &[Expr]) -> R<Value> {
        let values = || args.iter().map(|e| self.eval(e).map(Cow::into_owned)).collect::<R<Vec<_>>>();
        if let Some(key) = Expr::method_key(callee, &|name| self.is_local(name)) {
            if let Some(result) = self.methods.call(&key, &values()?) {
                return result;
            }
        }
        match callee {
            // `Some(x)`, `Tab::Home(x)`
            Expr::Var(name) if name.starts_with(char::is_uppercase) => Ok(constant(name, Some(values()?))),
            Expr::Path(parts) => Ok(constant(parts.last().map_or("", String::as_str), Some(values()?))),
            Expr::Attr(obj, name) => {
                let receiver = self.eval(obj)?;
                builtin(&receiver, name, &values()?).unwrap_or_else(|| {
                    Err(match Expr::method_key(callee, &|name| self.is_local(name)) {
                        Some(key) => format!(
                            "`{callee}()` isn't in this build of the studio: the studio can only call the methods the \
                             templates called when it was built. Rebuild it once (`wasm-pack build --target web` in `studio/`); key `{key}`"
                        ),
                        None => format!("the studio can't call `{callee}()`: it calls only methods of the model and its fields, not of loop variables"),
                    })
                })
            }
            _ => Err(format!("the studio can't call `{callee}()`")),
        }
    }

    fn binary(&self, op: BinOp, left: &Expr, right: &Expr) -> R<Value> {
        let l = self.eval(left)?;
        // `&&` / `||` evaluate the right side only when needed.
        match op {
            BinOp::And if !as_bool(&l)? => return Ok(Value::Bool(false)),
            BinOp::Or if as_bool(&l)? => return Ok(Value::Bool(true)),
            BinOp::And | BinOp::Or => return Ok(Value::Bool(as_bool(&*self.eval(right)?)?)),
            _ => {}
        }
        let r = self.eval(right)?;
        match op {
            BinOp::Eq => Ok(Value::Bool(equal(&l, &r)?)),
            BinOp::Ne => Ok(Value::Bool(!equal(&l, &r)?)),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ord = compare(&l, &r)?;
                Ok(Value::Bool(match op {
                    BinOp::Lt => ord.is_lt(),
                    BinOp::Le => ord.is_le(),
                    BinOp::Gt => ord.is_gt(),
                    _ => ord.is_ge(),
                }))
            }
            _ => arithmetic(op, &l, &r),
        }
    }
}

/// `value`'s field `name` (or tuple element `0`, `1`, ..).
fn field<'v>(value: &'v Value, name: &str) -> R<&'v Value> {
    match value {
        Value::Struct(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v).ok_or_else(|| format!("no field `{name}`")),
        Value::Tuple(items) => name.parse::<usize>().ok().and_then(|i| items.get(i)).ok_or_else(|| format!("no field `{name}` in a tuple")),
        other => Err(format!("a {} has no field `{name}`", other.kind())),
    }
}

/// A path or constructor call: `None`, `Some(x)`, `Tab::Home`, `Tab::Page(3)`.
fn constant(name: &str, args: Option<Vec<Value>>) -> Value {
    match (name, args) {
        ("None", None) => Value::Option(None),
        ("Some", Some(mut args)) if args.len() == 1 => Value::Option(Some(Box::new(args.remove(0)))),
        (name, None) => Value::Variant(name.to_string(), Box::new(Value::Unit)),
        (name, Some(mut args)) if args.len() == 1 => Value::Variant(name.to_string(), Box::new(args.remove(0))),
        (name, Some(args)) => Value::Variant(name.to_string(), Box::new(Value::Tuple(args))),
    }
}

/// The methods of standard types the studio runs itself.
fn builtin(receiver: &Value, name: &str, args: &[Value]) -> Option<R<Value>> {
    let usize = |n: usize| Value::Int(n as i128, IntType::U64);
    Some(Ok(match (receiver, name, args) {
        (Value::List(items) | Value::Tuple(items), "len", []) => usize(items.len()),
        (Value::Struct(fields), "len", []) => usize(fields.len()),
        (Value::Str(s), "len", []) => usize(s.len()),
        (Value::List(items), "is_empty", []) => Value::Bool(items.is_empty()),
        (Value::Struct(fields), "is_empty", []) => Value::Bool(fields.is_empty()),
        (Value::Str(s), "is_empty", []) => Value::Bool(s.is_empty()),
        (Value::Option(o), "is_some", []) => Value::Bool(o.is_some()),
        (Value::Option(o), "is_none", []) => Value::Bool(o.is_none()),
        (value, "to_string", []) => return Some(value.display().map(Value::Str)),
        (value, "clone" | "iter", []) => value.clone(),
        _ => return None,
    }))
}

fn as_bool(value: &Value) -> R<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(format!("a condition must be a bool, found a {}", other.kind())),
    }
}

/// Binds `pattern` to `value` if it matches.
fn bind(pattern: &Pattern, value: Value, scopes: &mut Vec<(String, Value)>) -> bool {
    match pattern {
        Pattern::Name(name) => {
            scopes.push((name.clone(), value));
            true
        }
        Pattern::Placeholder | Pattern::Rest(None) => true,
        Pattern::Rest(Some(name)) => {
            scopes.push((name.clone(), value));
            true
        }
        Pattern::Path(path) => match (path.last().map(String::as_str), &value) {
            (Some("None"), Value::Option(o)) => o.is_none(),
            (Some(name), Value::Variant(variant, _)) => name == variant,
            _ => false,
        },
        Pattern::Tuple(path, patterns) => {
            let items = match (path.last().map(String::as_str), value) {
                (None, Value::Tuple(items) | Value::List(items)) => items,
                (Some("Some"), Value::Option(Some(inner))) => vec![*inner],
                (Some(name), Value::Variant(variant, payload)) if *name == variant => match *payload {
                    Value::Tuple(items) if patterns.len() != 1 => items,
                    payload => vec![payload],
                },
                _ => return false,
            };
            bind_all(patterns, items, scopes)
        }
    }
}

fn bind_all(patterns: &[Pattern], items: Vec<Value>, scopes: &mut Vec<(String, Value)>) -> bool {
    match patterns.iter().position(|p| matches!(p, Pattern::Rest(_))) {
        None => {
            patterns.len() == items.len() && patterns.iter().zip(items).all(|(p, v)| bind(p, v, scopes))
        }
        Some(rest) => {
            let after = patterns.len() - rest - 1;
            if items.len() < rest + after {
                return false;
            }
            let mut items = items;
            let tail = items.split_off(items.len() - after);
            let middle = items.split_off(rest);
            patterns[..rest].iter().zip(items).all(|(p, v)| bind(p, v, scopes))
                && bind(&patterns[rest], Value::List(middle), scopes)
                && patterns[rest + 1..].iter().zip(tail).all(|(p, v)| bind(p, v, scopes))
        }
    }
}

fn equal(l: &Value, r: &Value) -> R<bool> {
    Ok(match (l, r) {
        (Value::Int(..) | Value::Float(..), Value::Int(..) | Value::Float(..)) => compare(l, r)?.is_eq(),
        (Value::Option(a), Value::Option(b)) => match (a, b) {
            (Some(a), Some(b)) => equal(a, b)?,
            (a, b) => a.is_none() && b.is_none(),
        },
        (Value::List(a) | Value::Tuple(a), Value::List(b) | Value::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).map(|(a, b)| equal(a, b)).collect::<R<Vec<_>>>()?.into_iter().all(|e| e)
        }
        (Value::Variant(a, pa), Value::Variant(b, pb)) => a == b && equal(pa, pb)?,
        (a, b) if std::mem::discriminant(a) == std::mem::discriminant(b) => a == b,
        (a, b) => return Err(format!("can't compare a {} with a {}", a.kind(), b.kind())),
    })
}

fn compare(l: &Value, r: &Value) -> R<std::cmp::Ordering> {
    let float = |v: &Value| match v {
        Value::Float(f, FloatType::F32) => *f as f32 as f64,
        Value::Float(f, _) => *f,
        Value::Int(i, _) => *i as f64,
        _ => f64::NAN,
    };
    Ok(match (l, r) {
        (Value::Int(a, _), Value::Int(b, _)) => a.cmp(b),
        (Value::Int(..) | Value::Float(..), Value::Int(..) | Value::Float(..)) => {
            // An `f32` compares with a literal at `f32` precision.
            let f32s = matches!(l, Value::Float(_, FloatType::F32)) || matches!(r, Value::Float(_, FloatType::F32));
            let (a, b) = if f32s { (float(l) as f32 as f64, float(r) as f32 as f64) } else { (float(l), float(r)) };
            a.partial_cmp(&b).ok_or("comparison with NaN")?
        }
        (Value::Str(a), Value::Str(b)) => a.cmp(b),
        (Value::Char(a), Value::Char(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (a, b) => return Err(format!("can't order a {} and a {}", a.kind(), b.kind())),
    })
}

fn int_range(ty: IntType) -> (i128, i128) {
    match ty {
        IntType::I8 => (i8::MIN.into(), i8::MAX.into()),
        IntType::I16 => (i16::MIN.into(), i16::MAX.into()),
        IntType::I32 | IntType::Untyped => (i32::MIN.into(), i32::MAX.into()),
        IntType::I64 => (i64::MIN.into(), i64::MAX.into()),
        IntType::I128 => (i128::MIN, i128::MAX),
        IntType::U8 => (0, u8::MAX.into()),
        IntType::U16 => (0, u16::MAX.into()),
        IntType::U32 => (0, u32::MAX.into()),
        IntType::U64 => (0, u64::MAX.into()),
        IntType::U128 => (0, i128::MAX),
    }
}

fn arithmetic(op: BinOp, l: &Value, r: &Value) -> R<Value> {
    match (l, r) {
        (Value::Int(a, ta), Value::Int(b, tb)) => {
            let ty = if *ta == IntType::Untyped { *tb } else { *ta };
            let (a, b) = (*a, *b);
            let result = match op {
                BinOp::Add => a.checked_add(b),
                BinOp::Sub => a.checked_sub(b),
                BinOp::Mul => a.checked_mul(b),
                BinOp::Div => a.checked_div(b),
                BinOp::Rem => a.checked_rem(b),
                BinOp::BitAnd => Some(a & b),
                BinOp::BitOr => Some(a | b),
                BinOp::BitXor => Some(a ^ b),
                BinOp::Shl => u32::try_from(b).ok().and_then(|b| a.checked_shl(b)),
                BinOp::Shr => u32::try_from(b).ok().and_then(|b| a.checked_shr(b)),
                _ => None,
            };
            let (min, max) = int_range(ty);
            match result {
                Some(v) if (min..=max).contains(&v) => Ok(Value::Int(v, ty)),
                _ => Err(format!("`{a} {} {b}` overflows (or divides by zero)", op.as_str())),
            }
        }
        (Value::Int(..) | Value::Float(..), Value::Int(..) | Value::Float(..)) => {
            let ty = match (l, r) {
                (Value::Float(_, FloatType::F32), _) | (_, Value::Float(_, FloatType::F32)) => FloatType::F32,
                (Value::Float(_, FloatType::F64), _) | (_, Value::Float(_, FloatType::F64)) => FloatType::F64,
                _ => FloatType::Untyped,
            };
            let get = |v: &Value| match v {
                Value::Float(f, _) => *f,
                Value::Int(i, _) => *i as f64,
                _ => 0.0,
            };
            let (a, b) = (get(l), get(r));
            // `f32` math rounds to `f32` after each operation.
            let value = if ty == FloatType::F32 {
                let (a, b) = (a as f32, b as f32);
                (match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Rem => a % b,
                    _ => return Err(format!("can't apply `{}` to floats", op.as_str())),
                }) as f64
            } else {
                match op {
                    BinOp::Add => a + b,
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Rem => a % b,
                    _ => return Err(format!("can't apply `{}` to floats", op.as_str())),
                }
            };
            Ok(Value::Float(value, ty))
        }
        (Value::Str(a), Value::Str(b)) if op == BinOp::Add => Ok(Value::Str(format!("{a}{b}"))),
        (a, b) => Err(format!("can't apply `{}` to a {} and a {}", op.as_str(), a.kind(), b.kind())),
    }
}

/// A number literal, typed by its suffix.
fn number(text: &str) -> R<Value> {
    let text = text.replace('_', "");
    let suffixes: [(&str, Option<IntType>, Option<FloatType>); 14] = [
        ("i8", Some(IntType::I8), None),
        ("i16", Some(IntType::I16), None),
        ("i32", Some(IntType::I32), None),
        ("i64", Some(IntType::I64), None),
        ("i128", Some(IntType::I128), None),
        ("isize", Some(IntType::I64), None),
        ("u8", Some(IntType::U8), None),
        ("u16", Some(IntType::U16), None),
        ("u32", Some(IntType::U32), None),
        ("u64", Some(IntType::U64), None),
        ("u128", Some(IntType::U128), None),
        ("usize", Some(IntType::U64), None),
        ("f32", None, Some(FloatType::F32)),
        ("f64", None, Some(FloatType::F64)),
    ];
    let hex = text.starts_with("0x") || text.starts_with("0o") || text.starts_with("0b");
    let (digits, int_ty, float_ty) = suffixes
        .iter()
        .find(|(s, _, f)| text.ends_with(s) && !(hex && f.is_some()))
        .map_or((text.as_str(), None, None), |(s, i, f)| (&text[..text.len() - s.len()], *i, *f));
    let bad = || format!("bad number `{text}`");
    if let Some((radix, rest)) = [("0x", 16), ("0o", 8), ("0b", 2)].iter().find_map(|(p, r)| digits.strip_prefix(p).map(|rest| (*r, rest))) {
        return Ok(Value::Int(i128::from_str_radix(rest, radix).map_err(|_| bad())?, int_ty.unwrap_or(IntType::Untyped)));
    }
    let is_float = float_ty.is_some() || digits.contains(['.', 'e', 'E']);
    if is_float {
        Ok(Value::Float(digits.parse().map_err(|_| bad())?, float_ty.unwrap_or(FloatType::Untyped)))
    } else {
        Ok(Value::Int(digits.parse().map_err(|_| bad())?, int_ty.unwrap_or(IntType::Untyped)))
    }
}

/// The text of a string or char literal, escapes resolved.
fn unescape(s: &str) -> R<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(c @ ('\\' | '"' | '\'')) => out.push(c),
            Some('u') => {
                let code: String = chars.by_ref().skip_while(|c| *c == '{').take_while(|c| *c != '}').collect();
                let c = u32::from_str_radix(&code, 16).ok().and_then(char::from_u32).ok_or_else(|| format!("bad escape `\\u{{{code}}}`"))?;
                out.push(c);
            }
            other => return Err(format!("unsupported escape `\\{}`", other.map_or(String::new(), String::from))),
        }
    }
    Ok(out)
}
