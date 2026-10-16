//! askama_parser's tree → xerune's statements and expressions. This is
//! where the supported subset is decided: whatever doesn't lower is an
//! error pointing at the template.

use askama_parser::{Expr as AExpr, Node, Target, WithSpan, node::CondTest};

use crate::expr::{BinOp, Cond, Expr, Pattern, RangeOp, UnaryOp};
use crate::source::{Ctx, Error};

/// A template statement, before the HTML around it is parsed.
#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    /// Literal template text (HTML), whitespace included.
    Lit(String),
    /// `{{ expr }}`, at its offset in the template.
    Expr(Expr, usize),
    If(Vec<Branch>, usize),
    For(Pattern, Expr, Vec<Stmt>, usize),
    Break(usize),
    Continue(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub cond: Option<Cond>,
    pub body: Vec<Stmt>,
}

impl Stmt {
    pub(crate) fn offset(&self) -> usize {
        match self {
            Stmt::Lit(_) => 0,
            Stmt::Expr(_, at) | Stmt::If(_, at) | Stmt::For(.., at) | Stmt::Break(at) | Stmt::Continue(at) => *at,
        }
    }
}

/// Writes statements back in template syntax.
impl std::fmt::Display for Stmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stmt::Lit(text) => f.write_str(text),
            Stmt::Expr(expr, _) => write!(f, "{{{{ {expr} }}}}"),
            Stmt::For(pattern, iter, body, _) => {
                write!(f, "{{% for {pattern} in {iter} %}}")?;
                body.iter().try_for_each(|s| write!(f, "{s}"))?;
                f.write_str("{% endfor %}")
            }
            Stmt::If(branches, _) => {
                for (i, branch) in branches.iter().enumerate() {
                    match (&branch.cond, i) {
                        (Some(cond), 0) => write!(f, "{{% if {cond} %}}")?,
                        (Some(cond), _) => write!(f, "{{% else if {cond} %}}")?,
                        (None, _) => f.write_str("{% else %}")?,
                    }
                    branch.body.iter().try_for_each(|s| write!(f, "{s}"))?;
                }
                f.write_str("{% endif %}")
            }
            Stmt::Break(_) => f.write_str("{% break %}"),
            Stmt::Continue(_) => f.write_str("{% continue %}"),
        }
    }
}

fn node_span<'a>(node: &Node<'a>) -> askama_parser::Span<'a> {
    match node {
        Node::Lit(n) => n.span(),
        Node::Comment(n) => n.span(),
        Node::Expr(_, n) => n.span(),
        Node::Call(n) => n.span(),
        Node::Let(n) => n.span(),
        Node::If(n) => n.span(),
        Node::Match(n) => n.span(),
        Node::Loop(n) => n.span(),
        Node::Extends(n) => n.span(),
        Node::BlockDef(n) => n.span(),
        Node::Include(n) => n.span(),
        Node::Import(n) => n.span(),
        Node::Macro(n) => n.span(),
        Node::Raw(n) => n.span(),
        Node::Break(n) => n.span(),
        Node::Continue(n) => n.span(),
        Node::FilterBlock(n) => n.span(),
    }
}

pub(crate) fn nodes(ctx: &Ctx<'_>, list: &[Node<'_>]) -> Result<Vec<Stmt>, Error> {
    let mut out = Vec::new();
    for node in list {
        let at = ctx.offset(node_span(node)).unwrap_or(0);
        match node {
            Node::Lit(lit) => out.push(Stmt::Lit(format!("{}{}{}", lit.lws, lit.val, lit.rws))),
            Node::Comment(_) => {}
            Node::Break(_) => out.push(Stmt::Break(at)),
            Node::Continue(_) => out.push(Stmt::Continue(at)),
            Node::Expr(_, e) => out.push(Stmt::Expr(expr(ctx, e)?, at)),
            Node::If(if_node) => {
                let mut branches = Vec::new();
                for branch in &if_node.branches {
                    let cond = match &branch.cond {
                        Some(c) => Some(cond(ctx, c, branch.span())?),
                        None => None,
                    };
                    branches.push(Branch { cond, body: nodes(ctx, &branch.nodes)? });
                }
                out.push(Stmt::If(branches, at));
            }
            Node::Loop(l) => {
                let pattern = pattern(ctx, &l.var, l.span())?;
                let iter = expr(ctx, &l.iter)?;
                if let Some(cond) = &l.cond {
                    return Err(ctx.error(cond.span(), "loop filters (`{% for x in xs if cond %}`) are not supported; wrap the body in `{% if %}` instead"));
                }
                if !l.else_nodes.is_empty() {
                    return Err(ctx.error(l.span(), "`{% else %}` inside `{% for %}` is not supported; check for emptiness with a separate `{% if %}`"));
                }
                out.push(Stmt::For(pattern, iter, nodes(ctx, &l.body)?, at));
            }
            other => {
                let what = match other {
                    Node::Call(_) => "`{% call %}`",
                    Node::Let(_) => "`{% let %}` / `{% set %}`",
                    Node::Match(_) => "`{% match %}`",
                    Node::Extends(_) => "`{% extends %}`",
                    Node::BlockDef(_) => "`{% block %}`",
                    Node::Include(_) => "`{% include %}` (could not be resolved)",
                    Node::Import(_) => "`{% import %}`",
                    Node::Macro(_) => "`{% macro %}`",
                    Node::Raw(_) => "`{% raw %}`",
                    Node::FilterBlock(_) => "`{% filter %}`",
                    _ => "this tag",
                };
                return Err(ctx.error(node_span(other), format_args!(
                    "{what} is not supported by xerune templates (supported: `{{{{ expr }}}}`, `{{% if %}}`/`{{% elif %}}`/`{{% else %}}`, `{{% for %}}`, `{{% break %}}`, `{{% continue %}}`, `{{% include %}}`)"
                )));
            }
        }
    }
    Ok(out)
}

fn cond(ctx: &Ctx<'_>, c: &CondTest<'_>, span: askama_parser::Span<'_>) -> Result<Cond, Error> {
    let pattern = match &c.target {
        Some(target) => Some(pattern(ctx, target, span)?),
        None => None,
    };
    Ok(Cond { pattern, expr: expr(ctx, &c.expr)? })
}

fn pattern(ctx: &Ctx<'_>, target: &Target<'_>, span: askama_parser::Span<'_>) -> Result<Pattern, Error> {
    let path = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();
    Ok(match target {
        Target::Name(name) => Pattern::Name(name.to_string()),
        Target::Placeholder(_) => Pattern::Placeholder,
        Target::Rest(rest) => Pattern::Rest(rest.map(str::to_string)),
        Target::Path(parts) => Pattern::Path(path(parts)),
        Target::Tuple(parts, targets) => Pattern::Tuple(
            path(parts),
            targets.iter().map(|t| pattern(ctx, t, span)).collect::<Result<_, _>>()?,
        ),
        other => return Err(ctx.error(span, format_args!("unsupported pattern `{other:?}`; only names, `_`, paths and tuple patterns like `Some(x)` / `(a, b)` are supported"))),
    })
}

fn exprs(ctx: &Ctx<'_>, list: &[WithSpan<'_, AExpr<'_>>]) -> Result<Vec<Expr>, Error> {
    list.iter().map(|e| expr(ctx, e)).collect()
}

fn boxed(ctx: &Ctx<'_>, e: &WithSpan<'_, AExpr<'_>>) -> Result<Box<Expr>, Error> {
    expr(ctx, e).map(Box::new)
}

pub(crate) fn expr(ctx: &Ctx<'_>, e: &WithSpan<'_, AExpr<'_>>) -> Result<Expr, Error> {
    let unsupported = || ctx.error(e.span(), "this expression is not supported in xerune templates");
    Ok(match &**e {
        AExpr::BoolLit(b) => Expr::Bool(*b),
        AExpr::NumLit(s, _) => Expr::Num(s.to_string()),
        AExpr::StrLit(s) => Expr::Str(s.content.to_string()),
        AExpr::CharLit(c) => Expr::Char(c.content.to_string()),
        AExpr::Var(name) => Expr::Var(name.to_string()),
        AExpr::Path(parts) => Expr::Path(parts.iter().map(|p| p.to_string()).collect()),
        AExpr::Array(elems) => Expr::Array(exprs(ctx, elems)?),
        AExpr::Tuple(elems) => Expr::Tuple(exprs(ctx, elems)?),
        AExpr::Attr(obj, attr) => {
            if matches!(&***obj, AExpr::Var("loop")) && !matches!(attr.name, "index" | "index0") {
                return Err(ctx.error(e.span(), format_args!("`loop.{}` is not supported; use `loop.index` or `loop.index0`", attr.name)));
            }
            Expr::Attr(boxed(ctx, obj)?, attr.name.to_string())
        }
        AExpr::Index(a, b) => Expr::Index(boxed(ctx, a)?, boxed(ctx, b)?),
        AExpr::BinOp(op, a, b) => {
            let (a, b) = (boxed(ctx, a)?, boxed(ctx, b)?);
            Expr::Binary(BinOp::from_askama(op).ok_or_else(unsupported)?, a, b)
        }
        AExpr::Unary(op, inner) => {
            let inner = boxed(ctx, inner)?;
            Expr::Unary(UnaryOp::from_askama(op).ok_or_else(unsupported)?, inner)
        }
        AExpr::Group(inner) => Expr::Group(boxed(ctx, inner)?),
        AExpr::Range(op, a, b) => {
            let a = a.as_ref().map(|a| boxed(ctx, a)).transpose()?;
            let b = b.as_ref().map(|b| boxed(ctx, b)).transpose()?;
            let op = match *op {
                ".." => RangeOp::Exclusive,
                "..=" => RangeOp::Inclusive,
                _ => return Err(unsupported()),
            };
            Expr::Range(op, a, b)
        }
        AExpr::Call { path, args, generics } => {
            if !generics.is_empty() {
                return Err(ctx.error(e.span(), "generic arguments on calls (turbofish) are not supported in templates"));
            }
            Expr::Call(boxed(ctx, path)?, exprs(ctx, args)?)
        }
        AExpr::Filter(f) => {
            if f.name != "format" {
                return Err(ctx.error(e.span(), format_args!(
                    "filter `|{}` is not supported; the only supported filter is `format`, e.g. `{{{{ \"{{:.1}}\"|format(value) }}}}`. Format the value in Rust (a method on your model works: `{{{{ self.my_value() }}}}`)",
                    f.name
                )));
            }
            if f.arguments.len() < 2 {
                return Err(ctx.error(e.span(), "`format` needs a format string and at least one argument: `{{ \"{}\"|format(value) }}`"));
            }
            Expr::Format(exprs(ctx, &f.arguments)?)
        }
        AExpr::Concat(_) => return Err(ctx.error(e.span(), "the `~` concatenation operator is not supported; use `\"{}{}\"|format(a, b)`")),
        AExpr::Try(_) => return Err(ctx.error(e.span(), "the `?` operator is not supported in templates")),
        _ => return Err(unsupported()),
    })
}
