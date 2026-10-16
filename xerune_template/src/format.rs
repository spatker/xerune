//! `"{:.1}"|format(v)`: Rust's `format!` at run time, for the values the
//! interpreter knows. Numbers go through Rust's own formatting, so the
//! digits are the ones the compiled template prints.

use crate::value::{FloatType, IntType, Value};

#[derive(Default)]
struct Spec {
    fill: Option<char>,
    align: Option<char>,
    plus: bool,
    alternate: bool,
    zero: bool,
    width: Option<usize>,
    precision: Option<usize>,
    kind: Option<char>,
}

/// `format!(fmt, args..)`.
pub(crate) fn format(fmt: &str, args: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let mut next_arg = 0;
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '}' => return Err(format!("unmatched `}}` in format string {fmt:?}")),
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => inner.push(c),
                        None => return Err(format!("unterminated `{{` in format string {fmt:?}")),
                    }
                }
                let (position, spec) = inner.split_once(':').unwrap_or((&inner, ""));
                let index = if position.is_empty() {
                    next_arg += 1;
                    next_arg - 1
                } else {
                    position.parse::<usize>().map_err(|_| format!("the studio supports only positional arguments in format strings, not `{{{position}}}`"))?
                };
                let value = args.get(index).ok_or_else(|| format!("format string {fmt:?} needs argument {index}, but there are {}", args.len()))?;
                out.push_str(&format_value(value, &parse_spec(spec)?)?);
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

fn parse_spec(spec: &str) -> Result<Spec, String> {
    let mut out = Spec::default();
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let is_align = |c: char| matches!(c, '<' | '^' | '>');
    if chars.len() >= 2 && is_align(chars[1]) {
        out.fill = Some(chars[0]);
        out.align = Some(chars[1]);
        i = 2;
    } else if chars.first().copied().is_some_and(is_align) {
        out.align = Some(chars[0]);
        i = 1;
    }
    if chars.get(i) == Some(&'+') {
        out.plus = true;
        i += 1;
    } else if chars.get(i) == Some(&'-') {
        i += 1;
    }
    if chars.get(i) == Some(&'#') {
        out.alternate = true;
        i += 1;
    }
    if chars.get(i) == Some(&'0') {
        out.zero = true;
        i += 1;
    }
    let number = |i: &mut usize| {
        let start = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        (start < *i).then(|| chars[start..*i].iter().collect::<String>().parse::<usize>().unwrap_or(0))
    };
    out.width = number(&mut i);
    if chars.get(i) == Some(&'.') {
        i += 1;
        out.precision = Some(number(&mut i).ok_or_else(|| format!("the studio supports only literal precisions in `{{:{spec}}}`"))?);
    }
    match &chars[i..] {
        [] => {}
        [kind @ ('?' | 'x' | 'X' | 'o' | 'b' | 'e' | 'E')] => out.kind = Some(*kind),
        _ => return Err(format!("the studio doesn't support the format spec `{{:{spec}}}`")),
    }
    Ok(out)
}

/// A float as Rust formats an `f32` or `f64`.
macro_rules! float {
    ($x:expr, $spec:expr) => {{
        let x = $x;
        match ($spec.kind, $spec.precision) {
            (None, None) => format!("{}", x),
            (None, Some(p)) => format!("{:.*}", p, x),
            (Some('?'), None) => format!("{:?}", x),
            (Some('?'), Some(p)) => format!("{:.*?}", p, x),
            (Some('e'), None) => format!("{:e}", x),
            (Some('e'), Some(p)) => format!("{:.*e}", p, x),
            (Some('E'), None) => format!("{:E}", x),
            (Some('E'), Some(p)) => format!("{:.*E}", p, x),
            (Some(kind), _) => return Err(format!("`{{:{kind}}}` doesn't format floats")),
        }
    }};
}

/// An integer as Rust formats its type (hex of a negative `i8` is two
/// digits, as the bits of an `i8`).
macro_rules! int {
    ($x:expr, $spec:expr) => {{
        let x = $x;
        match $spec.kind {
            None | Some('?') => format!("{}", x),
            Some('x') => format!("{:x}", x),
            Some('X') => format!("{:X}", x),
            Some('o') => format!("{:o}", x),
            Some('b') => format!("{:b}", x),
            Some('e') => format!("{:e}", x),
            Some('E') => format!("{:E}", x),
            Some(kind) => return Err(format!("`{{:{kind}}}` doesn't format integers")),
        }
    }};
}

fn format_value(value: &Value, spec: &Spec) -> Result<String, String> {
    let (body, numeric) = match value {
        Value::Float(x, FloatType::F32) => (float!(*x as f32, spec), true),
        Value::Float(x, _) => (float!(*x, spec), true),
        Value::Int(i, ty) => {
            let i = *i;
            let body = match ty {
                IntType::I8 => int!(i as i8, spec),
                IntType::I16 => int!(i as i16, spec),
                IntType::I32 | IntType::Untyped => int!(i as i32, spec),
                IntType::I64 => int!(i as i64, spec),
                IntType::I128 => int!(i, spec),
                IntType::U8 => int!(i as u8, spec),
                IntType::U16 => int!(i as u16, spec),
                IntType::U32 => int!(i as u32, spec),
                IntType::U64 => int!(i as u64, spec),
                IntType::U128 => int!(i as u128, spec),
            };
            (body, true)
        }
        Value::Str(s) => {
            let s = match spec.kind {
                Some('?') => format!("{:?}", s),
                None => s.clone(),
                Some(kind) => return Err(format!("`{{:{kind}}}` doesn't format strings")),
            };
            match spec.precision {
                Some(p) if spec.kind.is_none() => (s.chars().take(p).collect(), false),
                _ => (s, false),
            }
        }
        Value::Char(c) => (if spec.kind == Some('?') { format!("{c:?}") } else { c.to_string() }, false),
        Value::Bool(b) => (b.to_string(), false),
        other => (other.display()?, false),
    };
    // Sign and `0x`-style prefix, which zero padding goes after.
    let (mut sign, mut body) = match body.strip_prefix('-') {
        Some(rest) if numeric => ("-".to_string(), rest.to_string()),
        _ => (if spec.plus && numeric { "+" } else { "" }.to_string(), body),
    };
    if spec.alternate {
        match spec.kind {
            Some('x' | 'X') => sign.push_str("0x"),
            Some('o') => sign.push_str("0o"),
            Some('b') => sign.push_str("0b"),
            Some('?') => {}
            _ => {}
        }
    }
    let len = sign.chars().count() + body.chars().count();
    let width = spec.width.unwrap_or(0);
    if len >= width {
        return Ok(sign + &body);
    }
    let pad = width - len;
    if spec.zero && numeric {
        body = "0".repeat(pad) + &body;
        return Ok(sign + &body);
    }
    let fill = spec.fill.unwrap_or(' ');
    let text = sign + &body;
    let align = spec.align.unwrap_or(if numeric { '>' } else { '<' });
    let (before, after) = match align {
        '<' => (0, pad),
        '^' => (pad / 2, pad - pad / 2),
        _ => (pad, 0),
    };
    let fill_str = |n: usize| std::iter::repeat_n(fill, n).collect::<String>();
    Ok(fill_str(before) + &text + &fill_str(after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rust() {
        let f32v = Value::Float(2.5f32 as f64, FloatType::F32);
        let f32_third = Value::Float((1.0f32 / 3.0) as f64, FloatType::F32);
        let int = |i: i128, ty| Value::Int(i, ty);
        let cases: Vec<(&str, Vec<Value>, String)> = vec![
            ("{}", vec![f32_third.clone()], format!("{}", 1.0f32 / 3.0)),
            ("{:.1}", vec![f32v.clone()], format!("{:.1}", 2.5f32)),
            ("{:.2}%", vec![f32_third.clone()], format!("{:.2}%", 1.0f32 / 3.0)),
            ("{:>6.1}", vec![f32v.clone()], format!("{:>6.1}", 2.5f32)),
            ("{:06.1}", vec![Value::Float(-2.5, FloatType::F64)], format!("{:06.1}", -2.5f64)),
            ("{:+}", vec![int(5, IntType::U32)], format!("{:+}", 5u32)),
            ("{:02}:{:02}", vec![int(3, IntType::U64), int(7, IntType::U64)], format!("{:02}:{:02}", 3u64, 7u64)),
            ("{:#x}", vec![int(255, IntType::U8)], format!("{:#x}", 255u8)),
            ("{:x}", vec![int(-1, IntType::I8)], format!("{:x}", -1i8)),
            ("{:*^9}", vec![Value::Str("mid".into())], format!("{:*^9}", "mid")),
            ("{:?}", vec![Value::Str("a\"b".into())], format!("{:?}", "a\"b")),
            ("{1}-{0}", vec![int(1, IntType::I32), int(2, IntType::I32)], format!("{1}-{0}", 1, 2)),
            ("{{{}}}", vec![int(1, IntType::I32)], format!("{{{}}}", 1)),
            ("{:e}", vec![Value::Float(1234.5, FloatType::F64)], format!("{:e}", 1234.5f64)),
        ];
        for (fmt, args, expected) in cases {
            assert_eq!(format(fmt, &args).unwrap(), expected, "{fmt}");
        }
    }
}
