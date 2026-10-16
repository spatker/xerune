//! The template language of [xerune](https://github.com/spatker/xerune).
//!
//! A template is HTML with Askama's tags (`{{ expr }}`, `{% if %}`,
//! `{% for %}`, `{% include %}`), whose expressions are Rust. [`parse`]
//! reads one into a [`Template`]: a single tree of HTML elements and
//! template blocks, with everything xerune doesn't support rejected.
//! `#[derive(XeruneTemplate)]` compiles that tree to Rust; the studio is to
//! interpret the same tree in the browser, to preview edits without
//! recompiling, so the two can't disagree about the language.

mod expr;
mod lower;
mod source;
mod tree;

pub use expr::{BinOp, Cond, Expr, Pattern, RangeOp, UnaryOp};
pub use source::{Error, File, Loader};
pub use tree::{Attr, AttrBranch, AttrPart, Kind, Node, NodeId, Template, is_dynamic};

use source::Ctx;

/// Parses the template `root`, inlining its `{% include %}`s with `load`.
pub fn parse(root: File, load: &mut Loader<'_>) -> Result<Template, Error> {
    let (src, map) = source::expand_includes(root, load)?;
    let ctx = Ctx { src: &src, map: Some(&map) };
    let syntax = askama_parser::Syntax::default();
    let ast = askama_parser::Ast::from_str(&src, None, &syntax).map_err(|e| {
        let msg = e.message.as_deref().unwrap_or("could not parse");
        let note = if msg.starts_with("unknown node") {
            "\n  = note: conditions use Rust operators: `&&`, `||`, `!` (not `and`, `or`, `not`)"
        } else {
            ""
        };
        Error(format!("template syntax error: {msg}\n  --> {}{note}", map.describe(e.offset)))
    })?;
    let stmts = lower::nodes(&ctx, ast.nodes())?;
    tree::build(&ctx, &stmts)
}
