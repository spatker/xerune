//! Template expressions: the subset of Askama's (Rust) expressions xerune
//! supports. `Display` writes them back in template syntax.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Bool(bool),
    /// As written, with any suffix (`5000.0`, `10u32`).
    Num(String),
    /// The text between the quotes, escapes as written.
    Str(String),
    /// The text between the quotes, escapes as written.
    Char(String),
    /// A name: a model field, or a binding of `{% for %}` / `{% if let %}`.
    Var(String),
    /// `a::b`, e.g. an enum variant.
    Path(Vec<String>),
    Array(Vec<Expr>),
    Tuple(Vec<Expr>),
    /// `expr.name`: a field, or a method when called.
    Attr(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Range(RangeOp, Option<Box<Expr>>, Option<Box<Expr>>),
    Group(Box<Expr>),
    /// The callee (usually an `Attr`) and the arguments.
    Call(Box<Expr>, Vec<Expr>),
    /// `"{:.1}"|format(a, b)`: the format string, then its arguments.
    Format(Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    Deref,
    Ref,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Ge,
    Gt,
    Le,
    Lt,
    BitOr,
    BitXor,
    BitAnd,
    Shr,
    Shl,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeOp {
    /// `a..b`
    Exclusive,
    /// `a..=b`
    Inclusive,
}

/// A pattern of `{% for PAT in .. %}` or `{% if let PAT = .. %}`.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Name(String),
    /// `_`
    Placeholder,
    /// `None`, `Tab::Home`
    Path(Vec<String>),
    /// `(a, b)` (empty path) or `Some(x)`.
    Tuple(Vec<String>, Vec<Pattern>),
    /// `..`, or `name @ ..`
    Rest(Option<String>),
}

/// The test of an `{% if %}` / `{% elif %}`: a `bool` expression, or
/// `let PAT = EXPR`.
#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    pub pattern: Option<Pattern>,
    pub expr: Expr,
}

impl UnaryOp {
    /// The operator in Rust (and template) syntax.
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Not => "!",
            UnaryOp::Neg => "-",
            UnaryOp::Deref => "*",
            UnaryOp::Ref => "&",
        }
    }

    pub(crate) fn from_askama(op: &str) -> Option<Self> {
        Some(match op {
            "!" => UnaryOp::Not,
            "-" => UnaryOp::Neg,
            "*" => UnaryOp::Deref,
            "&" => UnaryOp::Ref,
            _ => return None,
        })
    }
}

impl BinOp {
    const ALL: [(BinOp, &'static str, &'static str); 18] = [
        (BinOp::Or, "||", "||"),
        (BinOp::And, "&&", "&&"),
        (BinOp::Eq, "==", "=="),
        (BinOp::Ne, "!=", "!="),
        (BinOp::Ge, ">=", ">="),
        (BinOp::Gt, ">", ">"),
        (BinOp::Le, "<=", "<="),
        (BinOp::Lt, "<", "<"),
        (BinOp::BitOr, "|", "bitor"),
        (BinOp::BitXor, "^", "xor"),
        (BinOp::BitAnd, "&", "bitand"),
        (BinOp::Shr, ">>", ">>"),
        (BinOp::Shl, "<<", "<<"),
        (BinOp::Add, "+", "+"),
        (BinOp::Sub, "-", "-"),
        (BinOp::Mul, "*", "*"),
        (BinOp::Div, "/", "/"),
        (BinOp::Rem, "%", "%"),
    ];

    /// The operator in Rust syntax.
    pub fn as_str(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(_, rust, _)| rust)
    }

    /// The operator in template syntax, where the bitwise ones are spelled
    /// as keywords (`|` would start a filter).
    pub fn template_str(self) -> &'static str {
        Self::ALL.iter().find(|(op, ..)| *op == self).map_or("", |(.., template)| template)
    }

    /// From askama_parser, which reports the Rust spelling.
    pub(crate) fn from_askama(op: &str) -> Option<Self> {
        Self::ALL.iter().find(|(_, rust, _)| *rust == op).map(|(op, ..)| *op)
    }
}

impl RangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            RangeOp::Exclusive => "..",
            RangeOp::Inclusive => "..=",
        }
    }
}

impl Pattern {
    /// The variables the pattern binds.
    pub fn names(&self, out: &mut Vec<String>) {
        match self {
            Pattern::Name(name) => out.push(name.clone()),
            Pattern::Tuple(_, patterns) => patterns.iter().for_each(|p| p.names(out)),
            Pattern::Rest(Some(name)) => out.push(name.clone()),
            _ => {}
        }
    }
}

fn join(f: &mut fmt::Formatter<'_>, items: &[impl fmt::Display]) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Bool(b) => write!(f, "{b}"),
            Expr::Num(n) => f.write_str(n),
            Expr::Str(s) => write!(f, "\"{s}\""),
            Expr::Char(c) => write!(f, "'{c}'"),
            Expr::Var(name) => f.write_str(name),
            Expr::Path(parts) => f.write_str(&parts.join("::")),
            Expr::Array(elems) => {
                f.write_str("[")?;
                join(f, elems)?;
                f.write_str("]")
            }
            Expr::Tuple(elems) => {
                f.write_str("(")?;
                join(f, elems)?;
                f.write_str(if elems.len() == 1 { ",)" } else { ")" })
            }
            Expr::Attr(obj, name) => write!(f, "{obj}.{name}"),
            Expr::Index(obj, key) => write!(f, "{obj}[{key}]"),
            Expr::Unary(op, inner) => write!(f, "{}{inner}", op.as_str()),
            Expr::Binary(op, left, right) => write!(f, "{left} {} {right}", op.template_str()),
            Expr::Range(op, left, right) => {
                if let Some(left) = left {
                    write!(f, "{left}")?;
                }
                f.write_str(op.as_str())?;
                if let Some(right) = right {
                    write!(f, "{right}")?;
                }
                Ok(())
            }
            Expr::Group(inner) => write!(f, "({inner})"),
            Expr::Call(callee, args) => {
                write!(f, "{callee}(")?;
                join(f, args)?;
                f.write_str(")")
            }
            Expr::Format(args) => {
                write!(f, "{}|format(", args[0])?;
                join(f, &args[1..])?;
                f.write_str(")")
            }
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Name(name) => f.write_str(name),
            Pattern::Placeholder => f.write_str("_"),
            Pattern::Path(path) => f.write_str(&path.join("::")),
            Pattern::Tuple(path, patterns) => {
                write!(f, "{}(", path.join("::"))?;
                join(f, patterns)?;
                f.write_str(if path.is_empty() && patterns.len() == 1 { ",)" } else { ")" })
            }
            Pattern::Rest(Some(name)) => write!(f, "{name} @ .."),
            Pattern::Rest(None) => f.write_str(".."),
        }
    }
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.pattern {
            Some(pattern) => write!(f, "let {pattern} = {}", self.expr),
            None => write!(f, "{}", self.expr),
        }
    }
}
