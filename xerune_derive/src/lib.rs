use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
use askama_parser::{Ast, Syntax, Node, Expr, Target, node::CondTest};
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::cell::RefCell;
use askama_parser::Span;
use xerune_css::selector::PseudoElement;

// ---------------------------------------------------------------------------
// Diagnostics
//
// The template compiler is deeply recursive and reports problems by panicking
// with a `TemplateError` payload (see `bail`). `derive_xerune_template` catches
// the unwind and turns it into a regular `compile_error!`, so users see a
// `file:line:col` message instead of "proc-macro derive panicked".
// ---------------------------------------------------------------------------

/// Panic payload for user-facing template errors.
struct TemplateError(String);

/// A contiguous chunk of the include-expanded template that came from one file.
struct Segment {
    start: usize,
    file: usize,
    file_offset: usize,
}

/// Maps offsets in the include-expanded template back to the original files.
#[derive(Default)]
struct SourceMap {
    /// (display path, file contents)
    files: Vec<(String, String)>,
    segments: Vec<Segment>,
}

impl SourceMap {
    fn add_file(&mut self, display_path: String, content: String) -> usize {
        self.files.push((display_path, content));
        self.files.len() - 1
    }

    fn describe_file_offset(&self, file: usize, offset: usize) -> String {
        let (path, content) = &self.files[file];
        let offset = offset.min(content.len());
        let before = &content[..offset];
        let line_no = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let line_end = content[offset..].find('\n').map_or(content.len(), |i| offset + i);
        let line = &content[line_start..line_end];
        let col = content[line_start..offset].chars().count() + 1;
        let gutter = " ".repeat(line_no.to_string().len());
        format!(
            "{path}:{line_no}:{col}\n{gutter} |\n{line_no} | {line}\n{gutter} | {caret:>width$}",
            caret = "^",
            width = col,
        )
    }

    /// Describe an offset into the include-expanded template.
    fn describe(&self, offset: usize) -> String {
        match self.segments.iter().rev().find(|s| s.start <= offset) {
            Some(seg) => self.describe_file_offset(seg.file, seg.file_offset + (offset - seg.start)),
            None => format!("<template>@{offset}"),
        }
    }
}

thread_local! {
    /// The include-expanded template being compiled, and its source map.
    static CURRENT_TEMPLATE: RefCell<Option<(&'static str, SourceMap)>> = const { RefCell::new(None) };
}

/// Abort template compilation with `msg`, without a source location.
fn fail(msg: impl std::fmt::Display) -> ! {
    std::panic::panic_any(TemplateError(msg.to_string()))
}

/// Abort template compilation with `msg`, pointing at `span` in the template.
fn bail(span: Span<'_>, msg: impl std::fmt::Display) -> ! {
    let location = CURRENT_TEMPLATE.with(|cur| {
        let cur = cur.borrow();
        let (src, map) = cur.as_ref()?;
        let offset = span.offset_from(src)?;
        Some(map.describe(offset))
    });
    match location {
        Some(loc) => fail(format_args!("{msg}\n  --> {loc}")),
        None => fail(msg),
    }
}

fn node_span<'a>(node: &Node<'a>) -> Span<'a> {
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

/// Reject everything the code generator cannot handle, while spans are still
/// available. After this pass the generator may assume a supported subset.
fn validate_nodes(nodes: &[Node<'_>]) {
    for node in nodes {
        match node {
            Node::Lit(_) | Node::Comment(_) | Node::Break(_) | Node::Continue(_) => {}
            Node::Expr(_, expr) => validate_expr(expr),
            Node::If(if_node) => {
                for branch in &if_node.branches {
                    if let Some(cond) = &branch.cond {
                        if let Some(target) = &cond.target {
                            validate_target(target, branch.span());
                        }
                        validate_expr(&cond.expr);
                    }
                    validate_nodes(&branch.nodes);
                }
            }
            Node::Loop(l) => {
                validate_target(&l.var, l.span());
                validate_expr(&l.iter);
                if let Some(cond) = &l.cond {
                    bail(cond.span(), "loop filters (`{% for x in xs if cond %}`) are not supported; wrap the body in `{% if %}` instead");
                }
                if !l.else_nodes.is_empty() {
                    bail(l.span(), "`{% else %}` inside `{% for %}` is not supported; check for emptiness with a separate `{% if %}`");
                }
                validate_nodes(&l.body);
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
                bail(node_span(other), format_args!(
                    "{what} is not supported by xerune templates (supported: `{{{{ expr }}}}`, `{{% if %}}`/`{{% elif %}}`/`{{% else %}}`, `{{% for %}}`, `{{% break %}}`, `{{% continue %}}`, `{{% include %}}`)"
                ));
            }
        }
    }
}

fn validate_target(target: &Target<'_>, span: Span<'_>) {
    match target {
        Target::Name(_) | Target::Placeholder(_) | Target::Rest(_) | Target::Path(_) => {}
        Target::Tuple(_, targets) => targets.iter().for_each(|t| validate_target(t, span)),
        other => bail(span, format_args!("unsupported pattern `{other:?}`; only names, `_`, paths and tuple patterns like `Some(x)` / `(a, b)` are supported")),
    }
}

fn validate_expr(expr: &askama_parser::WithSpan<'_, Expr<'_>>) {
    match &**expr {
        Expr::BoolLit(_) | Expr::NumLit(..) | Expr::StrLit(_) | Expr::CharLit(_) | Expr::Var(_) | Expr::Path(_) => {}
        Expr::Array(elems) | Expr::Tuple(elems) => elems.iter().for_each(validate_expr),
        Expr::Attr(obj, attr) => {
            if matches!(&***obj, Expr::Var("loop")) && !matches!(attr.name, "index" | "index0") {
                bail(expr.span(), format_args!("`loop.{}` is not supported; use `loop.index` or `loop.index0`", attr.name));
            }
            validate_expr(obj);
        }
        Expr::Index(a, b) | Expr::BinOp(_, a, b) => {
            validate_expr(a);
            validate_expr(b);
        }
        Expr::Unary(_, inner) | Expr::Group(inner) => validate_expr(inner),
        Expr::Range(_, a, b) => {
            a.iter().for_each(|e| validate_expr(e));
            b.iter().for_each(|e| validate_expr(e));
        }
        Expr::Call { path, args, generics } => {
            if !generics.is_empty() {
                bail(expr.span(), "generic arguments on calls (turbofish) are not supported in templates");
            }
            validate_expr(path);
            args.iter().for_each(validate_expr);
        }
        Expr::Filter(f) => {
            if f.name != "format" {
                bail(expr.span(), format_args!(
                    "filter `|{}` is not supported; the only supported filter is `format`, e.g. `{{{{ \"{{:.1}}\"|format(value) }}}}`. Format the value in Rust (a method on your model works: `{{{{ self.my_value() }}}}`)",
                    f.name
                ));
            }
            if f.arguments.len() < 2 {
                bail(expr.span(), "`format` needs a format string and at least one argument: `{{ \"{}\"|format(value) }}`");
            }
            f.arguments.iter().for_each(validate_expr);
        }
        Expr::Concat(_) => bail(expr.span(), "the `~` concatenation operator is not supported; use `\"{}{}\"|format(a, b)`"),
        Expr::Try(_) => bail(expr.span(), "the `?` operator is not supported in templates"),
        _ => bail(expr.span(), "this expression is not supported in xerune templates"),
    }
}

/// Escape a re-serialized template tag so it survives inside an HTML attribute
/// value quoted with `quote`. html5ever decodes the entities again, so the
/// attribute-level template parser sees the original text.
fn escape_for_attr(s: &str, quote: char) -> String {
    let s = s.replace('&', "&amp;");
    match quote {
        '"' => s.replace('"', "&quot;"),
        _ => s.replace('\'', "&#39;"),
    }
}

// Stringify Askama targets (patterns). `validate_target` restricts the variants.
fn format_target(target: &Target<'_>) -> String {
    match target {
        Target::Name(name) => name.to_string(),
        Target::Placeholder(_) => "_".to_string(),
        Target::Path(path) => path.join("::"),
        Target::Tuple(path, targets) => {
            let inner: Vec<String> = targets.iter().map(format_target).collect();
            match (path.is_empty(), inner.len()) {
                // A one-element tuple pattern needs the trailing comma.
                (true, 1) => format!("({},)", inner[0]),
                (true, _) => format!("({})", inner.join(", ")),
                (false, _) => format!("{}({})", path.join("::"), inner.join(", ")),
            }
        }
        Target::Rest(_) => "..".to_string(),
        other => panic!("Unsupported target variant: {:?}", other),
    }
}

/// Collect the variable names a pattern binds, so the body treats them as
/// locals rather than model fields (`self.name`).
fn target_names(target: &Target<'_>, names: &mut HashSet<String>) {
    match target {
        Target::Name(name) => {
            names.insert(name.to_string());
        }
        Target::Tuple(_, targets) => targets.iter().for_each(|t| target_names(t, names)),
        Target::Rest(rest) => {
            if let Some(name) = **rest {
                names.insert(name.to_string());
            }
        }
        _ => {}
    }
}

/// A piece of an inline text run: literal text or a `{{ }}` value.
enum TextPiece {
    Literal(String),
    Expr(proc_macro2::TokenStream),
}

/// `handle` as part of a text run, if it is text or a `{{ }}` placeholder.
fn text_piece(handle: &Handle, local_vars: &HashSet<String>, dynamic_exprs: &HashMap<usize, &Expr<'_>>) -> Option<TextPiece> {
    match &handle.data {
        NodeData::Text { contents } => Some(TextPiece::Literal(contents.borrow().to_string())),
        NodeData::Element { name, attrs, .. } if name.local.as_ref() == "template" => {
            let attrs = attrs.borrow();
            let id = attrs.iter().find(|a| a.name.local.as_ref() == "expr-id")?;
            let expr = dynamic_exprs.get(&id.value.to_string().parse::<usize>().ok()?)?;
            let tokens: proc_macro2::TokenStream = format_expr(expr, local_vars).parse().unwrap();
            Some(TextPiece::Expr(tokens))
        }
        _ => None,
    }
}

/// Compiles sibling nodes. Consecutive text and `{{ }}` values form one
/// text node, like one run of text in a browser, so it wraps as a whole
/// and keeps the spaces between its pieces.
fn compile_children<'a>(
    children: &[Handle],
    rulesets: &[MediaRuleSet],
    local_vars: &mut HashSet<String>,
    dynamic_exprs: &HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &HashMap<usize, &'a Node<'a>>,
) -> Vec<proc_macro2::TokenStream> {
    let mut code = Vec::new();
    let mut run: Vec<TextPiece> = Vec::new();
    let flush = |run: &mut Vec<TextPiece>, code: &mut Vec<proc_macro2::TokenStream>| {
        let pieces = core::mem::take(run);
        let has_content = pieces.iter().any(|p| match p {
            TextPiece::Literal(t) => !t.trim().is_empty(),
            TextPiece::Expr(_) => true,
        });
        if !has_content {
            return;
        }
        if let [TextPiece::Literal(text)] = pieces.as_slice() {
            code.push(quote! { builder.append_text_node(measurer, parent, &parent_style, #text); });
            return;
        }
        let pushes = pieces.iter().map(|p| match p {
            TextPiece::Literal(t) => quote! { text_run.push_str(#t); },
            TextPiece::Expr(e) => quote! { text_run.push_str(&(#e).to_display_string()); },
        });
        code.push(quote! {
            {
                use ::xerune::__private::ToDisplayString;
                let mut text_run = String::new();
                #(#pushes)*
                builder.append_text_node(measurer, parent, &parent_style, &text_run);
            }
        });
    };
    for child in children {
        if let Some(piece) = text_piece(child, local_vars, dynamic_exprs) {
            run.push(piece);
            continue;
        }
        flush(&mut run, &mut code);
        code.push(compile_dom_node(child, rulesets, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
    }
    flush(&mut run, &mut code);
    code
}

fn get_node_children(handle: &Handle) -> Vec<Handle> {
    if let NodeData::Element { name, template_contents, .. } = &handle.data {
        if name.local.as_ref() == "template" {
            if let Some(contents) = template_contents.borrow().as_ref() {
                return contents.children.borrow().clone();
            }
        }
    }
    handle.children.borrow().clone()
}

fn reconstruct_expr(expr: &Expr<'_>) -> String {
    match expr {
        Expr::BoolLit(b) => format!("{}", b),
        Expr::NumLit(s, _) => s.to_string(),
        Expr::StrLit(s) => format!("\"{}\"", s.content),
        Expr::CharLit(c) => format!("'{}'", c.content),
        Expr::Var(name) => name.to_string(),
        Expr::Path(parts) => parts.join("::"),
        Expr::Array(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| reconstruct_expr(&**e)).collect();
            format!("[{}]", inner.join(", "))
        }
        Expr::Attr(obj, attr) => {
            format!("{}.{}", reconstruct_expr(&**obj), attr.name)
        }
        Expr::Index(obj, key) => {
            format!("{}[{}]", reconstruct_expr(&**obj), reconstruct_expr(&**key))
        }
        Expr::Unary(op, inner) => {
            format!("{}{}", op, reconstruct_expr(&**inner))
        }
        Expr::BinOp(op, left, right) => {
            format!("{} {} {}", reconstruct_expr(&**left), op, reconstruct_expr(&**right))
        }
        Expr::Range(op, left, right) => {
            let left = left.as_ref().map(|e| reconstruct_expr(&**e)).unwrap_or_default();
            let right = right.as_ref().map(|e| reconstruct_expr(&**e)).unwrap_or_default();
            format!("{}{}{}", left, op, right)
        }
        Expr::Group(inner) => {
            format!("({})", reconstruct_expr(&**inner))
        }
        Expr::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| reconstruct_expr(&**e)).collect();
            format!("({})", inner.join(", "))
        }
        Expr::Call { path, args, .. } => {
            let inner: Vec<String> = args.iter().map(|e| reconstruct_expr(&**e)).collect();
            format!("{}({})", reconstruct_expr(&**path), inner.join(", "))
        }
        Expr::Filter(f) => {
            let input = reconstruct_expr(&f.arguments[0]);
            if f.arguments.len() > 1 {
                let args: Vec<String> = f.arguments[1..].iter().map(|e| reconstruct_expr(e)).collect();
                format!("{}|{}({})", input, f.name, args.join(", "))
            } else {
                format!("{}|{}", input, f.name)
            }
        }
        _ => "".to_string(),
    }
}

fn reconstruct_target(target: &Target<'_>) -> String {
    match target {
        Target::Name(name) => name.to_string(),
        Target::Placeholder(_) => "_".to_string(),
        Target::Path(path) => path.join("::"),
        Target::Tuple(path, targets) => {
            let inner: Vec<String> = targets.iter().map(reconstruct_target).collect();
            format!("{}({})", path.join("::"), inner.join(", "))
        }
        Target::Rest(_) => "..".to_string(),
        _ => "_".to_string(),
    }
}

fn reconstruct_cond_test(cond: &CondTest<'_>) -> String {
    let expr_str = reconstruct_expr(&*cond.expr);
    if let Some(ref target) = cond.target {
        format!("let {} = {}", reconstruct_target(target), expr_str)
    } else {
        expr_str
    }
}

fn reconstruct_node(node: &Node<'_>) -> String {
    match node {
        Node::Lit(lit) => format!("{}{}{}", lit.lws, lit.val, lit.rws),
        Node::Comment(c) => format!("{{#{}#}}", c.content),
        Node::Expr(_, expr) => {
            format!("{{{{ {} }}}}", reconstruct_expr(expr))
        }
        Node::Loop(loop_box) => {
            let body: Vec<String> = loop_box.body.iter().map(reconstruct_node).collect();
            format!("{{% for {} in {} %}}{}{{% endfor %}}",
                reconstruct_target(&loop_box.var),
                reconstruct_expr(&loop_box.iter),
                body.join("")
            )
        }
        Node::If(if_node) => {
            let mut s = String::new();
            for (i, branch_with_span) in if_node.branches.iter().enumerate() {
                let branch = &**branch_with_span;
                let body: Vec<String> = branch.nodes.iter().map(reconstruct_node).collect();
                if i == 0 {
                    if let Some(ref cond) = branch.cond {
                        s.push_str(&format!("{{% if {} %}}", reconstruct_cond_test(cond)));
                    }
                } else if let Some(ref cond) = branch.cond {
                    s.push_str(&format!("{{% else if {} %}}", reconstruct_cond_test(cond)));
                } else {
                    s.push_str("{% else %}");
                }
                s.push_str(&body.join(""));
            }
            s.push_str("{% endif %}");
            s
        }
        Node::Break(_) => "{% break %}".to_string(),
        Node::Continue(_) => "{% continue %}".to_string(),
        _ => "".to_string(),
    }
}

/// Askama spells the bitwise operators as keywords.
fn rust_binop(op: &str) -> &str {
    match op {
        "bitand" => "&",
        "bitor" => "|",
        "xor" => "^",
        other => other,
    }
}

// Stringify Askama expressions
fn format_expr(expr: &Expr<'_>, local_vars: &HashSet<String>) -> String {
    match expr {
        Expr::BoolLit(b) => format!("{}", b),
        Expr::NumLit(s, _) => s.to_string(),
        Expr::StrLit(s) => format!("\"{}\"", s.content),
        Expr::CharLit(c) => format!("'{}'", c.content),
        Expr::Var(name) => {
            if name == &"self" {
                "self".to_string()
            } else if local_vars.contains(*name) {
                name.to_string()
            } else {
                format!("self.{}", name)
            }
        }
        Expr::Path(parts) => parts.join("::"),
        Expr::Array(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| format_expr(&**e, local_vars)).collect();
            format!("[{}]", inner.join(", "))
        }
        Expr::Attr(obj, attr) => {
            let obj_str = format_expr(&**obj, local_vars);
            if obj_str == "loop" {
                match attr.name {
                    "index" => "(_loop_item_index + 1)".to_string(),
                    "index0" => "_loop_item_index".to_string(),
                    _ => panic!("Unsupported loop attribute: {}", attr.name),
                }
            } else {
                format!("{}.{}", obj_str, attr.name)
            }
        }
        Expr::Index(obj, key) => {
            format!("{}[{}]", format_expr(&**obj, local_vars), format_expr(&**key, local_vars))
        }
        Expr::Unary(op, inner) => {
            format!("{}{}", op, format_expr(&**inner, local_vars))
        }
        Expr::BinOp(op, left, right) => {
            format!("{} {} {}", format_expr(&**left, local_vars), rust_binop(op), format_expr(&**right, local_vars))
        }
        Expr::Range(op, left, right) => {
            let left = left.as_ref().map(|e| format_expr(&**e, local_vars)).unwrap_or_default();
            let right = right.as_ref().map(|e| format_expr(&**e, local_vars)).unwrap_or_default();
            format!("({}{}{})", left, op, right)
        }
        Expr::Group(inner) => {
            format!("({})", format_expr(&**inner, local_vars))
        }
        Expr::Tuple(elems) => {
            let inner: Vec<String> = elems.iter().map(|e| format_expr(&**e, local_vars)).collect();
            format!("({})", inner.join(", "))
        }
        Expr::Call { path, args, .. } => {
            let inner: Vec<String> = args.iter().map(|e| format_expr(&**e, local_vars)).collect();
            format!("{}({})", format_expr(&**path, local_vars), inner.join(", "))
        }
        Expr::Filter(f) => {
            if f.name == "format" {
                if f.arguments.len() >= 2 {
                    let fmt_str = format_expr(&f.arguments[0], local_vars);
                    let args: Vec<String> = f.arguments[1..].iter().map(|arg| format_expr(arg, local_vars)).collect();
                    format!("format!({}, {})", fmt_str, args.join(", "))
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        }
        other => panic!("Unhandled expression: {:?}", other),
    }
}

fn format_cond_test(cond: &CondTest<'_>, local_vars: &HashSet<String>) -> String {
    let expr_str = format_expr(&*cond.expr, local_vars);
    if let Some(ref target) = cond.target {
        // Borrow the scrutinee: templates only get `&self`, so binding by
        // value would try to move out of the model.
        format!("let {} = &({})", format_target(target), expr_str)
    } else {
        as_bool(&expr_str)
    }
}

/// `expr` as a `bool`, accepting `&bool` too: loop variables are references
/// into the iterated collection, so `{% for on in flags %}{% if on %}` sees
/// a `&bool`.
fn as_bool(expr: &str) -> String {
    format!("(*::core::borrow::Borrow::<bool>::borrow(&({})))", expr)
}

/// Emit a template node that sits inside an HTML start tag. Inside a quoted
/// attribute value it is re-serialized (and escaped) for the attribute-level
/// compiler; anywhere else in the tag html5ever would shred it into bogus
/// attribute names, so that is a hard error.
fn push_tag_level(html: &mut String, node: &Node<'_>, in_quote: Option<char>, what: &str) {
    match in_quote {
        Some(q) => html.push_str(&escape_for_attr(&reconstruct_node(node), q)),
        None => bail(node_span(node), format_args!(
            "{what} can only appear inside a quoted attribute value. Write `class=\"{{% if c %}}a{{% else %}}b{{% endif %}}\"` \
             instead of `{{% if c %}}class=\"a\"{{% else %}}class=\"b\"{{% endif %}}`. \
             (The only tag-level conditional supported is `{{% if c %}}checked{{% endif %}}`.)"
        )),
    }
}

/// A condition as a plain `bool` expression (usable in `&&` chains and under
/// `!`). `if let PAT = EXPR` becomes `matches!(&(EXPR), PAT)`.
fn cond_test_as_bool(cond: &CondTest<'_>, local_vars: &HashSet<String>) -> String {
    let expr_str = format_expr(&*cond.expr, local_vars);
    match &cond.target {
        Some(target) => format!(
            "{{ #[allow(unused_variables)] let m = matches!(&({}), {}); m }}",
            expr_str,
            format_target(target)
        ),
        None => as_bool(&expr_str),
    }
}

/// Where the template's HTML scanner is: in text, inside a tag (where
/// quotes delimit attribute values), or in an HTML comment (where neither
/// tags nor quotes count: an apostrophe in a comment opens nothing).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Markup {
    Text,
    Tag,
    Comment,
}

// Generate HTML with placeholders
fn preprocess_nodes<'a>(
    nodes: &'a [Node<'a>],
    dynamic_counter: &mut usize,
    dynamic_exprs: &mut HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &mut HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &mut HashMap<usize, &'a Node<'a>>,
    markup: &mut Markup,
    in_quote: &mut Option<char>,
) -> String {
    let mut html = String::new();
    for node in nodes {
        match node {
            Node::Lit(lit) => {
                let val = format!("{}{}{}", lit.lws, lit.val, lit.rws);
                for (i, c) in val.char_indices() {
                    match *markup {
                        Markup::Comment => {
                            if c == '>' && val[..i].ends_with("--") {
                                *markup = Markup::Text;
                            }
                        }
                        Markup::Tag => {
                            if let Some(quote_char) = *in_quote {
                                if c == quote_char {
                                    *in_quote = None;
                                }
                            } else if c == '"' || c == '\'' {
                                *in_quote = Some(c);
                            } else if c == '>' {
                                *markup = Markup::Text;
                            } else if c == '-' && val[..i].ends_with("<!-") {
                                *markup = Markup::Comment;
                            }
                        }
                        Markup::Text => {
                            if c == '<' {
                                *markup = Markup::Tag;
                            }
                        }
                    }
                }
                html.push_str(&val);
            }
            Node::Comment(_) => {}
            Node::Expr(_, expr) => {
                if *markup == Markup::Tag {
                    push_tag_level(&mut html, node, *in_quote, "template expressions");
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_exprs.insert(id, &**expr);
                    html.push_str(&format!("<template expr-id=\"{}\"></template>", id));
                }
            }
            Node::Loop(loop_box) => {
                if *markup == Markup::Tag {
                    push_tag_level(&mut html, node, *in_quote, "`{% for %}`");
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_loops.insert(id, node);
                    html.push_str(&format!("<template loop-id=\"{}\">", id));
                    html.push_str(&preprocess_nodes(&loop_box.body, dynamic_counter, dynamic_exprs, dynamic_loops, dynamic_ifs, markup, in_quote));
                    html.push_str("</template>");
                }
            }
            Node::If(if_node) => {
                if *markup == Markup::Tag {
                    // `{% if cond %}checked{% endif %}` is the one tag-level
                    // conditional we support; it becomes `checked="{{ cond }}"`.
                    let checked_cond = match if_node.branches.as_slice() {
                        [branch] => match (&branch.cond, branch.nodes.as_slice()) {
                            (Some(cond), [Node::Lit(lit)]) if cond.target.is_none() && lit.val.trim() == "checked" => Some(&*cond.expr),
                            _ => None,
                        },
                        _ => None,
                    };
                    match (in_quote.is_none(), checked_cond) {
                        (true, Some(expr)) => {
                            let cond = escape_for_attr(&reconstruct_expr(expr), '"');
                            html.push_str(&format!(" checked=\"{{{{ {} }}}}\"", cond));
                        }
                        _ => push_tag_level(&mut html, node, *in_quote, "`{% if %}`"),
                    }
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_ifs.insert(id, node);
                    html.push_str(&format!("<template if-id=\"{}\">", id));
                    for (branch_idx, branch_with_span) in if_node.branches.iter().enumerate() {
                        let branch = &**branch_with_span;
                        html.push_str(&format!("<template branch-id=\"{}\" branch=\"{}\">", id, branch_idx));
                        html.push_str(&preprocess_nodes(&branch.nodes, dynamic_counter, dynamic_exprs, dynamic_loops, dynamic_ifs, markup, in_quote));
                        html.push_str("</template>");
                    }
                    html.push_str("</template>");
                }
            }
            Node::Break(_) => {
                if *markup == Markup::Tag {
                    push_tag_level(&mut html, node, *in_quote, "`{% break %}`");
                } else {
                    html.push_str("<template break></template>");
                }
            }
            Node::Continue(_) => {
                if *markup == Markup::Tag {
                    push_tag_level(&mut html, node, *in_quote, "`{% continue %}`");
                } else {
                    html.push_str("<template continue></template>");
                }
            }
            _ => {}
        }
    }
    html
}

// Generate code for Askama nodes within attribute string interpolation
fn generate_attr_string_code(nodes: &[Node<'_>], local_vars: &HashSet<String>) -> proc_macro2::TokenStream {
    let has_complex = nodes.iter().any(|node| !matches!(node, Node::Lit(_) | Node::Expr(_, _)));
    if !has_complex && !nodes.is_empty() {
        let mut format_str = String::new();
        let mut format_args = Vec::new();
        for node in nodes {
            match node {
                Node::Lit(lit) => {
                    let val = format!("{}{}{}", lit.lws, lit.val, lit.rws);
                    let escaped = val.replace("{", "{{").replace("}", "}}");
                    format_str.push_str(&escaped);
                }
                Node::Expr(_, expr) => {
                    let expr_str = format_expr(&**expr, local_vars);
                    let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                    format_str.push_str("{}");
                    format_args.push(expr_tokens);
                }
                _ => unreachable!(),
            }
        }
        return quote! {
            format!(#format_str, #(#format_args),*)
        };
    }

    let mut parts = Vec::new();
    for node in nodes {
        match node {
            Node::Lit(lit) => {
                let val = format!("{}{}{}", lit.lws, lit.val, lit.rws);
                parts.push(quote! { #val });
            }
            Node::Expr(_, expr) => {
                let expr_str = format_expr(&**expr, local_vars);
                let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                parts.push(quote! { &*format!("{}", #expr_tokens) });
            }
            Node::If(if_node) => {
                let mut branches_code = proc_macro2::TokenStream::new();
                for (i, branch_with_span) in if_node.branches.iter().enumerate() {
                    let branch = &**branch_with_span;
                    let mut branch_local_vars = local_vars.clone();
                    if let Some(target) = branch.cond.as_ref().and_then(|c| c.target.as_ref()) {
                        target_names(target, &mut branch_local_vars);
                    }
                    let body_tokens = generate_attr_string_code(&branch.nodes, &branch_local_vars);
                    if let Some(ref cond) = branch.cond {
                        let cond_str = format_cond_test(cond, local_vars);
                        let cond_tokens: proc_macro2::TokenStream = cond_str.parse().unwrap();
                        if i == 0 {
                            branches_code.extend(quote! {
                                if #cond_tokens {
                                    s.push_str(&#body_tokens);
                                }
                            });
                        } else {
                            branches_code.extend(quote! {
                                else if #cond_tokens {
                                    s.push_str(&#body_tokens);
                                }
                            });
                        }
                    } else {
                        branches_code.extend(quote! {
                            else {
                                s.push_str(&#body_tokens);
                            }
                        });
                    }
                }
                parts.push(quote! { &*({
                    let mut s = String::new();
                    #branches_code
                    s
                }) });
            }
            Node::Comment(_) => {}
            other => fail(format_args!(
                "only `{{{{ expr }}}}` and `{{% if %}}` are supported inside attribute values, found `{}`; \
                 build the value in Rust instead (e.g. a method on your model)",
                reconstruct_node(other)
            )),
        }
    }
    if parts.is_empty() {
        quote! { "".to_string() }
    } else {
        quote! {
            [#(#parts),*].join("")
        }
    }
}

// Recursively compile RcDom nodes into Rust builder calls
struct MacroElementWrapper {
    handle: Handle,
    active_classes: HashSet<String>,
    /// Whether a `checked="{{ ... }}"` is taken as checked in this
    /// matching pass (a literal `checked` speaks for itself).
    checked: bool,
    /// Stands for elements only known at runtime: a `{% for %}`/`{% if %}`
    /// wrapper, or what comes before or after a loop's or branch's body.
    opaque: bool,
}

impl MacroElementWrapper {
    fn new(handle: Handle) -> Self {
        let opaque = is_template_element(&handle);
        MacroElementWrapper { active_classes: get_static_classes(&handle), handle, checked: false, opaque }
    }

    /// The element sibling `step` (-1 or 1) nodes away. Inside a loop's or
    /// branch's body, past its first or last element is whatever the
    /// template runs into, unknown until runtime.
    fn sibling(&self, step: isize) -> Option<Self> {
        let parent_weak = self.handle.parent.take();
        let parent = parent_weak.as_ref().and_then(|weak| weak.upgrade());
        self.handle.parent.set(parent_weak);
        let parent = parent?;
        let siblings = parent.children.borrow();
        let index = siblings.iter().position(|child| std::rc::Rc::ptr_eq(child, &self.handle))?;
        let element = |sibling: &Handle| matches!(sibling.data, NodeData::Element { .. }).then(|| MacroElementWrapper::new(sibling.clone()));
        let found = if step < 0 {
            siblings[..index].iter().rev().find_map(element)
        } else {
            siblings[index + 1..].iter().find_map(element)
        };
        found.or_else(|| {
            let owner = TEMPLATE_OWNERS.with(|m| m.borrow().get(&(std::rc::Rc::as_ptr(&parent) as usize)).cloned())?;
            Some(MacroElementWrapper::new(owner))
        })
    }
}

/// Classes an element always has: literal names outside any `{% if %}`.
/// Parses the template syntax rather than splitting on whitespace, which
/// would read the words of `{% if on %}` as classes `if` and `on`.
fn get_static_classes(handle: &Handle) -> HashSet<String> {
    let mut static_classes = HashSet::new();
    if let NodeData::Element { ref attrs, .. } = handle.data {
        if let Some(attr) = attrs.borrow().iter().find(|a| a.name.local.as_ref() == "class") {
            for (class, cond) in get_classes_and_conditions(attr.value.as_ref(), &HashSet::new()) {
                if cond.is_none() {
                    static_classes.insert(class);
                }
            }
        }
    }
    static_classes
}

thread_local! {
    /// html5ever keeps `<template>` children in a separate content fragment
    /// with no link back to the `<template>` element. Maps fragment pointer to
    /// its `<template>` so selector matching can walk up through
    /// `{% for %}`/`{% if %}` wrappers.
    static TEMPLATE_OWNERS: RefCell<HashMap<usize, Handle>> = RefCell::new(HashMap::new());
}

fn record_template_owners(handle: &Handle) {
    if let NodeData::Element { template_contents, .. } = &handle.data {
        if let Some(contents) = template_contents.borrow().as_ref() {
            TEMPLATE_OWNERS.with(|m| m.borrow_mut().insert(std::rc::Rc::as_ptr(contents) as usize, handle.clone()));
        }
    }
    for child in get_node_children(handle).iter() {
        record_template_owners(child);
    }
}

fn dom_parent(handle: &Handle) -> Option<Handle> {
    let parent_weak = handle.parent.take();
    let parent = parent_weak.as_ref().and_then(|weak| weak.upgrade());
    handle.parent.set(parent_weak);
    let parent = parent?;
    // Hop from a template content fragment to its `<template>` element.
    let owner = TEMPLATE_OWNERS.with(|m| m.borrow().get(&(std::rc::Rc::as_ptr(&parent) as usize)).cloned());
    Some(owner.unwrap_or(parent))
}

fn is_template_element(handle: &Handle) -> bool {
    matches!(&handle.data, NodeData::Element { name, .. } if name.local.as_ref() == "template")
}

impl xerune_css::selector::Element for MacroElementWrapper {
    // The nearest real element, skipping the `<template>` wrappers that stand
    // in for `{% for %}`/`{% if %}`. Siblings still see the wrappers, which
    // are opaque: which iteration or branch comes first is only known at
    // runtime.
    fn parent(&self) -> Option<Self> {
        let mut parent = dom_parent(&self.handle)?;
        while is_template_element(&parent) {
            parent = dom_parent(&parent)?;
        }
        matches!(parent.data, NodeData::Element { .. }).then(|| MacroElementWrapper::new(parent))
    }

    fn prev_sibling(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn tag(&self) -> &str {
        match &self.handle.data {
            NodeData::Element { name, .. } => name.local.as_ref(),
            _ => "",
        }
    }

    fn attribute(&self, name: &str) -> Option<std::borrow::Cow<'_, str>> {
        let value = attribute_value(&self.handle, name)?;
        Some(std::borrow::Cow::Owned(match name {
            // Only the classes active in this matching pass count. The raw
            // attribute is template source: `{% if on %}on{% endif %}`
            // contains the word `on` whether or not the class is applied.
            "class" => self.active_classes.iter().cloned().collect::<Vec<_>>().join(" "),
            "checked" if is_dynamic(&value) => return self.checked.then(|| std::borrow::Cow::Borrowed("")),
            _ => value,
        }))
    }

    fn has_class(&self, class: &str) -> bool {
        self.active_classes.contains(class)
    }

    fn is_empty(&self) -> bool {
        self.handle.children.borrow().iter().all(|child| match &child.data {
            NodeData::Text { contents } => contents.borrow().trim().is_empty(),
            NodeData::Comment { .. } => true,
            _ => false,
        })
    }

    fn state(&self, state: xerune_css::selector::State) -> bool {
        // `checked="false"` is unchecked, as `parse_attributes_generic`
        // reads it.
        state == xerune_css::selector::State::Checked && match attribute_value(&self.handle, "checked") {
            Some(v) if is_dynamic(&v) => self.checked,
            Some(v) => v != "false",
            None => false,
        }
    }

    fn is_opaque(&self) -> bool {
        self.opaque
    }
}

fn attribute_value(handle: &Handle, name: &str) -> Option<String> {
    match &handle.data {
        NodeData::Element { attrs, .. } => attrs.borrow().iter().find(|a| a.name.local.as_ref() == name).map(|a| a.value.to_string()),
        _ => None,
    }
}

fn is_dynamic(value: &str) -> bool {
    value.contains("{{") || value.contains("{%")
}

fn get_classes_and_conditions(
    class_attr_val: &str,
    local_vars: &HashSet<String>,
) -> Vec<(String, Option<String>)> {
    let syntax = askama_parser::Syntax::default();
    let Ok(ast) = askama_parser::Ast::from_str(class_attr_val, None, &syntax) else {
        return class_attr_val.split_whitespace().map(|c| (c.to_string(), None)).collect();
    };

    fn walk_nodes(
        nodes: &[askama_parser::Node<'_>],
        cond: Option<String>,
        local_vars: &HashSet<String>,
        res: &mut Vec<(String, Option<String>)>,
        class_attr_val: &str,
    ) {
        for node in nodes {
            match node {
                askama_parser::Node::Lit(lit) => {
                    let val = format!("{}{}{}", lit.lws, lit.val, lit.rws);
                    for cls in val.split_whitespace() {
                        if !cls.is_empty() {
                            res.push((cls.to_string(), cond.clone()));
                        }
                    }
                }
                askama_parser::Node::If(if_node) => {
                    // A branch applies when its own test holds and every
                    // earlier test in the chain failed (that is what makes
                    // `elif`/`else` exclusive).
                    let mut earlier: Vec<String> = Vec::new();
                    for branch_with_span in &if_node.branches {
                        let branch = &**branch_with_span;
                        let mut parts: Vec<String> = cond.iter().cloned().collect();
                        parts.extend(earlier.iter().map(|c| format!("!({})", c)));
                        if let Some(ref c) = branch.cond {
                            let test = cond_test_as_bool(c, local_vars);
                            parts.push(test.clone());
                            earlier.push(test);
                        }
                        let branch_cond = match parts.len() {
                            0 => None,
                            1 => parts.pop(),
                            _ => Some(parts.iter().map(|p| format!("({})", p)).collect::<Vec<_>>().join(" && ")),
                        };
                        walk_nodes(&branch.nodes, branch_cond, local_vars, res, class_attr_val);
                    }
                }
                askama_parser::Node::Expr(..) => fail(format_args!(
                    "class names produced by `{{{{ ... }}}}` (in `class=\"{class_attr_val}\"`) can't be styled: CSS is matched \
                     at compile time. Use literal class names behind conditions instead: \
                     `class=\"{{% if selected %}}active{{% endif %}}\"`"
                )),
                _ => {}
            }
        }
    }

    let mut res = Vec::new();
    walk_nodes(ast.nodes(), None, local_vars, &mut res, class_attr_val);
    res
}

struct MatchedRule {
    /// The pseudo-element the declarations are for, if any.
    part: Option<xerune_css::ControlPart>,
    declarations: Vec<(String, String)>,
    condition: Option<String>,
    /// Guard expression (`xerune::css::media::matches_query(...)`) when the
    /// rule was declared inside an `@media` block; None = always applies.
    media_guard: Option<proc_macro2::TokenStream>,
}

/// A parsed stylesheet region together with the `@media` guard it appeared under.
struct MediaRuleSet {
    sheet: xerune_css::sheet::StyleSheet,
    /// Some(expr) => wrap rule application in `if expr`; None => always.
    guard: Option<proc_macro2::TokenStream>,
}

/// Splits stylesheet text into `(query, css)` regions; `query == None` marks
/// top-level rules. Nested `@media` blocks are flattened by AND-ing their
/// queries.
fn split_media_css(css: &str) -> Vec<(Option<String>, String)> {
    use xerune_css::media::Segment;
    let mut out = Vec::new();
    for segment in xerune_css::media::split_segments(css) {
        match segment {
            Segment::Base(text) => out.push((None, text.to_string())),
            Segment::Media { query, inner } => {
                for (sub_query, sub_css) in split_media_css(inner) {
                    match sub_query {
                        None => out.push((Some(query.to_string()), sub_css)),
                        Some(sq) => out.push((Some(format!("{} and {}", query, sq)), sub_css)),
                    }
                }
            }
        }
    }
    out
}

/// Result of compiling an `@media` query text to a runtime guard.
enum MediaGuard {
    /// Query always matches (`all`, `screen`, or empty) — no guard emitted.
    Always,
    /// Query can match: a `matches_query` call.
    Guard(proc_macro2::TokenStream),
}

/// Compiles an `@media` query into a guard evaluated against
/// `xerune::screen`'s viewport when the view is built, and whether part of
/// it can never match (an unknown feature or bad syntax).
fn compile_media_guard(text: &str) -> (MediaGuard, bool) {
    use xerune_css::media::Feature;
    let query = xerune_css::media::parse_query(text);
    // A part with only a media type matches every viewport.
    if query.groups.iter().any(|g| !g.negated && g.features.is_empty()) {
        return (MediaGuard::Always, query.unsupported);
    }
    let groups = query.groups.iter().map(|group| {
        let negated = group.negated;
        let features = group.features.iter().map(|f| match *f {
            Feature::MinWidth(v) => quote! { ::xerune::__private::Feature::MinWidth(#v) },
            Feature::MaxWidth(v) => quote! { ::xerune::__private::Feature::MaxWidth(#v) },
            Feature::MinHeight(v) => quote! { ::xerune::__private::Feature::MinHeight(#v) },
            Feature::MaxHeight(v) => quote! { ::xerune::__private::Feature::MaxHeight(#v) },
            Feature::Landscape => quote! { ::xerune::__private::Feature::Landscape },
            Feature::Portrait => quote! { ::xerune::__private::Feature::Portrait },
        });
        quote! { ::xerune::__private::FeatureGroup { negated: #negated, features: &[#(#features),*] } }
    });
    (MediaGuard::Guard(quote! { ::xerune::__private::matches_query(&[#(#groups),*]) }), query.unsupported)
}

fn compile_dom_node<'a>(
    handle: &Handle,
    rulesets: &[MediaRuleSet],
    local_vars: &mut HashSet<String>,
    dynamic_exprs: &HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &HashMap<usize, &'a Node<'a>>,
) -> proc_macro2::TokenStream {
    match &handle.data {
        NodeData::Document => {
            let children_code = compile_children(&get_node_children(handle), rulesets, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs);
            quote! {
                #(#children_code)*
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if tag == "style" || tag == "script" || tag == "head" {
                return quote! {};
            }

            if tag == "template" {
                let attrs_ref = attrs.borrow();
                
                // 1. Template expression
                let expr_id_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "expr-id");
                if let Some(attr) = expr_id_attr {
                    let id = attr.value.to_string().parse::<usize>().unwrap();
                    let expr = dynamic_exprs.get(&id).unwrap();
                    let expr_str = format_expr(expr, local_vars);
                    let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                    return quote! {
                        {
                            use ::xerune::__private::ToDisplayString;
                            let text_val = (#expr_tokens).to_display_string().into_owned();
                            builder.append_text_node(measurer, parent, &parent_style, &text_val);
                        }
                    };
                }

                // 2. Loop
                let loop_id_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "loop-id");
                if let Some(attr) = loop_id_attr {
                    let id = attr.value.to_string().parse::<usize>().unwrap();
                    let loop_node = dynamic_loops.get(&id).unwrap();
                    if let Node::Loop(ref loop_box) = **loop_node {
                        let var_str = format_target(&loop_box.var);
                        let iter_str = match &*loop_box.iter {
                            // Ranges are iterators already; `(0..n).iter()` doesn't exist.
                            Expr::Range(..) => format_expr(&loop_box.iter, local_vars),
                            _ => format!("({}).iter()", format_expr(&loop_box.iter, local_vars)),
                        };
                        
                        let mut loop_local_vars = local_vars.clone();
                        target_names(&loop_box.var, &mut loop_local_vars);
                        loop_local_vars.insert("_loop_item_index".to_string());
                        loop_local_vars.insert("loop".to_string());
                        
                        let children_code = compile_children(&get_node_children(handle), rulesets, &mut loop_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs);
                        
                        let var_tokens: proc_macro2::TokenStream = var_str.parse().unwrap();
                        let iter_tokens: proc_macro2::TokenStream = iter_str.parse().unwrap();

                        return quote! {
                            {
                                let parent = parent;
                                for (mut _loop_item_index, #var_tokens) in #iter_tokens.enumerate() {
                                    #(#children_code)*
                                }
                            }
                        };
                    }
                }

                // 3. Template If
                let if_id_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "if-id");
                if let Some(attr) = if_id_attr {
                    let id = attr.value.to_string().parse::<usize>().unwrap();
                    let if_node = dynamic_ifs.get(&id).unwrap();
                    if let Node::If(ref if_struct) = **if_node {
                        let mut if_code = proc_macro2::TokenStream::new();
                        for (branch_idx, branch_with_span) in if_struct.branches.iter().enumerate() {
                            let branch = &**branch_with_span;
                            let branch_handle = {
                                let children = get_node_children(handle);
                                children.iter().find(|child| {
                                    if let NodeData::Element { name: child_name, attrs: child_attrs, .. } = &child.data {
                                        if child_name.local.as_ref() == "template" {
                                            let child_attrs_ref = child_attrs.borrow();
                                            let b_id_attr = child_attrs_ref.iter().find(|a| a.name.local.as_ref() == "branch-id");
                                            let b_num_attr = child_attrs_ref.iter().find(|a| a.name.local.as_ref() == "branch");
                                            if let (Some(b_id), Some(b_num)) = (b_id_attr, b_num_attr) {
                                                return b_id.value.to_string().parse::<usize>().unwrap() == id
                                                    && b_num.value.to_string().parse::<usize>().unwrap() == branch_idx;
                                            }
                                        }
                                    }
                                    false
                                }).cloned()
                            };

                            if let Some(bh) = branch_handle {
                                // `{% if let Some(x) = ... %}` binds `x` inside this branch only.
                                let mut branch_local_vars = local_vars.clone();
                                if let Some(target) = branch.cond.as_ref().and_then(|c| c.target.as_ref()) {
                                    target_names(target, &mut branch_local_vars);
                                }
                                let children_code = compile_children(&get_node_children(&bh), rulesets, &mut branch_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs);
                                
                                if let Some(ref cond) = branch.cond {
                                    let cond_str = format_cond_test(cond, local_vars);
                                    let cond_tokens: proc_macro2::TokenStream = cond_str.parse().unwrap();
                                    if branch_idx == 0 {
                                        if_code.extend(quote! {
                                            if #cond_tokens {
                                                #(#children_code)*
                                            }
                                        });
                                    } else {
                                        if_code.extend(quote! {
                                            else if #cond_tokens {
                                                #(#children_code)*
                                            }
                                        });
                                    }
                                } else {
                                    if_code.extend(quote! {
                                        else {
                                            #(#children_code)*
                                        }
                                    });
                                }
                            }
                        }
                        return if_code;
                    }
                }

                // 4. Break
                if attrs_ref.iter().any(|a| a.name.local.as_ref() == "break") {
                    return quote! { break; };
                }

                // 5. Continue
                if attrs_ref.iter().any(|a| a.name.local.as_ref() == "continue") {
                    return quote! { continue; };
                }

                return quote! {};
            }

            // Standard Element styling logic
            let class_attr = attrs.borrow().iter().find(|a| a.name.local.as_ref() == "class").map(|a| a.value.to_string());
            let mut static_classes = HashSet::new();
            let mut conditional_classes = Vec::new();

            if let Some(ref class_val) = class_attr {
                let parsed_classes = get_classes_and_conditions(class_val, local_vars);
                for (cls, cond_opt) in parsed_classes {
                    if let Some(cond) = cond_opt {
                        conditional_classes.push((cls, cond));
                    } else {
                        static_classes.insert(cls);
                    }
                }
            }

            // A `checked="{{ ... }}"` (what `{% if c %}checked{% endif %}`
            // becomes) is checked at runtime unless it is "false", like
            // `parse_attributes_generic` reads it; `:checked` rules apply
            // under that condition.
            let dynamic_checked = attrs.borrow().iter().position(|a| a.name.local.as_ref() == "checked" && is_dynamic(a.value.as_ref()));
            let checked_condition = dynamic_checked.map(|_| "_dyn_checked".to_string());

            let has_parts = matches!(tag, "input" | "progress");
            let mut matched_rules = Vec::new();
            for rs in rulesets {
                for rule in &rs.sheet.rules {
                    let mut wrapper = MacroElementWrapper::new(handle.clone());
                    wrapper.active_classes = static_classes.clone();
                    // Only form controls have parts (`*::-moz-range-thumb`).
                    let part = match rule.selector.pseudo_element() {
                        None => None,
                        Some(PseudoElement::Part(part)) if has_parts || !part.of_form_control() => Some(part),
                        Some(_) => continue,
                    };
                    let declarations: Vec<(String, String)> = rule.declarations.iter()
                        .map(|d| (d.name.clone(), d.value.clone()))
                        .collect();
                    let mut push = |condition: Option<String>| matched_rules.push(MatchedRule {
                        part,
                        declarations: declarations.clone(),
                        condition,
                        media_guard: rs.guard.clone(),
                    });

                    if rule.selector.matches(&wrapper) {
                        push(None);
                        continue;
                    }
                    for (cls, cond) in &conditional_classes {
                        let mut active = static_classes.clone();
                        active.insert(cls.clone());
                        wrapper.active_classes = active;
                        wrapper.checked = false;
                        if rule.selector.matches(&wrapper) {
                            push(Some(cond.clone()));
                        } else if let Some(checked) = &checked_condition {
                            wrapper.checked = true;
                            if rule.selector.matches(&wrapper) {
                                push(Some(format!("({cond}) && ({checked})")));
                            }
                        }
                    }
                    if let Some(checked) = &checked_condition {
                        wrapper.active_classes = static_classes.clone();
                        wrapper.checked = true;
                        if rule.selector.matches(&wrapper) {
                            push(Some(checked.clone()));
                        }
                    }
                }
            }

            let mut rule_applications = Vec::new();
            for rule in matched_rules {
                let (names, values): (Vec<_>, Vec<_>) = rule.declarations.into_iter().unzip();
                let mut body = match rule.part {
                    None => quote! { el.apply(&[#((#names, #values)),*]); },
                    Some(part) => {
                        let part = syn::Ident::new(&format!("{part:?}"), proc_macro2::Span::call_site());
                        quote! { el.apply_part(::xerune::__private::ControlPart::#part, &[#((#names, #values)),*]); }
                    }
                };
                if let Some(cond) = rule.condition {
                    let cond_tokens: proc_macro2::TokenStream = cond.parse().unwrap();
                    body = quote! {
                        if #cond_tokens {
                            #body
                        }
                    };
                }
                if let Some(guard) = rule.media_guard {
                    body = quote! {
                        if #guard {
                            #body
                        }
                    };
                }
                rule_applications.push(body);
            }

            // Create attribute constructor
            let mut static_attrs = Vec::new();
            let mut dynamic_attrs = Vec::new();
            let mut dynamic_vars = Vec::new();
            let mut click = quote! {};
            for (idx, attr) in attrs.borrow().iter().enumerate() {
                let key = attr.name.local.as_ref();
                let val = attr.value.as_ref();
                if key == "data-on-click" {
                    click = compile_click(tag, val, local_vars);
                    continue;
                }
                if val.contains("{%") || val.contains("{{") {
                    let syntax = Syntax::default();
                    let val_ast = Ast::from_str(val, None, &syntax).unwrap_or_else(|e| fail(format_args!(
                        "failed to parse template syntax in attribute `{key}=\"{val}\"` of <{tag}>: {}",
                        e.message.as_deref().unwrap_or("syntax error")
                    )));
                    let val_tokens = generate_attr_string_code(val_ast.nodes(), local_vars);
                    let var_name = syn::Ident::new(&format!("_dyn_attr_{}", idx), proc_macro2::Span::call_site());
                    dynamic_vars.push(quote! {
                        let #var_name: String = #val_tokens;
                    });
                    dynamic_attrs.push(quote! { (std::borrow::Cow::Borrowed(#key), std::borrow::Cow::Owned(#var_name)) });
                } else {
                    static_attrs.push(quote! { (std::borrow::Cow::Borrowed(#key), std::borrow::Cow::Borrowed(#val)) });
                }
            }

            if let Some(idx) = dynamic_checked {
                let var_name = syn::Ident::new(&format!("_dyn_attr_{}", idx), proc_macro2::Span::call_site());
                dynamic_vars.push(quote! {
                    let _dyn_checked: bool = #var_name != "false";
                });
            }

            // A literal input `type` picks the widget defaults up front, so
            // the CSS rules below apply on top of them.
            let static_input_type = attrs.borrow().iter()
                .find(|a| tag == "input" && a.name.local.as_ref() == "type" && !a.value.contains("{{") && !a.value.contains("{%"))
                .map(|a| a.value.to_string());
            let element_style = match static_input_type {
                Some(input_type) => quote! { ::xerune::__private::ElementStyle::new_input(#input_type, &parent_style) },
                None => quote! { ::xerune::__private::ElementStyle::new(#tag, &parent_style) },
            };

            let child_compilation = compile_children(&get_node_children(handle), rulesets, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs);

            quote! {
                let (el, node) = {
                    #(#dynamic_vars)*
                    let attrs_slice: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'_, str>)] = &[
                        #(#static_attrs,)*
                        #(#dynamic_attrs),*
                    ];
                    let mut el = #element_style;
                    // Match and apply CSS rules
                    #(#rule_applications)*
                    #click
                    let node = builder.begin_element(&mut el, parent, attrs_slice, message_validator);
                    (el, node)
                };
                {
                    let parent = node;
                    let parent_style = &el.style;
                    #(#child_compilation)*
                }
                builder.finish_element(el, node, &parent_style);
            }
        }
        NodeData::Text { contents } => {
            let text = contents.borrow().to_string();
            if text.trim().is_empty() {
                return quote! {};
            }
            quote! {
                builder.append_text_node(measurer, parent, &parent_style, #text);
            }
        }
        _ => quote! {},
    }
}

/// The message a `data-on-click` sends. `prefix:{{ expr }}` calls the
/// variant's constructor generated by `#[derive(XeruneMessage)]`, so the
/// argument's type is checked by the compiler. Anything else (`add`,
/// `digit:7`, text around the expression) is parsed with `FromStr` when the
/// view is built, after the name was checked at compile time.
fn compile_click(tag: &str, val: &str, local_vars: &HashSet<String>) -> proc_macro2::TokenStream {
    let message_type = quote! { <Self as ::xerune::Model>::Message };
    if !val.contains("{{") && !val.contains("{%") {
        return quote! { el.on_click_text::<#message_type>(#val); };
    }
    let syntax = Syntax::default();
    let ast = Ast::from_str(val, None, &syntax).unwrap_or_else(|e| fail(format_args!(
        "failed to parse template syntax in attribute `data-on-click=\"{val}\"` of <{tag}>: {}",
        e.message.as_deref().unwrap_or("syntax error")
    )));
    if let [Node::Lit(prefix), Node::Expr(_, expr)] = ast.nodes() {
        if prefix.lws.is_empty() && prefix.rws.is_empty() && !prefix.val.is_empty() {
            let constructor = message_constructor(prefix.val);
            let expr: proc_macro2::TokenStream = format_expr(expr, local_vars).parse().unwrap();
            return quote! {
                el.on_click(#message_type::#constructor(::xerune::__private::message_arg(&(#expr))));
            };
        }
    }
    let text = generate_attr_string_code(ast.nodes(), local_vars);
    quote! { el.on_click_text::<#message_type>(&#text); }
}

// Find style content from HTML preprocessed templates
fn extract_css_from_html(html: &str) -> String {
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    fn walk(handle: &Handle, css: &mut String) {
        if let NodeData::Element { name, .. } = &handle.data {
            if name.local.as_ref() == "style" {
                for child in get_node_children(handle).iter() {
                    if let NodeData::Text { contents } = &child.data {
                        css.push_str(&contents.borrow());
                        css.push('\n');
                    }
                }
            }
        }
        for child in get_node_children(handle).iter() {
            walk(child, css);
        }
    }
    
    let mut css = String::new();
    walk(&dom.document, &mut css);
    css
}

/// The `@keyframes` blocks of `css`, with comments removed.
fn extract_keyframes(css: &str) -> String {
    let mut stripped = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        stripped.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    stripped.push_str(rest);

    let mut out = String::new();
    let mut rest = stripped.as_str();
    while let Some(start) = rest.find("@keyframes") {
        let block = &rest[start..];
        let Some(open) = block.find('{') else { break };
        let mut depth = 0;
        let mut end = block.len();
        for (i, c) in block[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push_str(&block[..end]);
        out.push('\n');
        rest = &block[end..];
    }
    out
}

// Helper to find a template file dynamically in parent dir, example templates dirs, root templates dir, or manifest dir
/// Finds a template, trying in order: next to the including template (for
/// `{% include %}`), the crate root, the crate's `templates/` directory, and
/// the workspace root (so crates of a workspace can share templates).
/// Returns its full path and a short name for messages.
fn find_template_file(
    path_str: &str,
    cargo_manifest_dir: &std::path::Path,
    parent_dir: Option<&std::path::Path>,
) -> Result<(std::path::PathBuf, String), String> {
    let workspace = workspace_root(cargo_manifest_dir);
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    candidates.extend(parent_dir.map(|dir| dir.join(path_str)));
    candidates.push(cargo_manifest_dir.join(path_str));
    candidates.push(cargo_manifest_dir.join("templates").join(path_str));
    candidates.extend(workspace.as_deref().filter(|ws| *ws != cargo_manifest_dir).map(|ws| ws.join(path_str)));

    let Some(found) = candidates.iter().find(|c| c.is_file()) else {
        let tried: Vec<String> = candidates.iter().map(|c| format!("  {}", c.display())).collect();
        return Err(format!("template `{path_str}` not found; looked at:\n{}", tried.join("\n")));
    };
    let short = [Some(cargo_manifest_dir), workspace.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|root| found.strip_prefix(root).ok())
        .unwrap_or(found)
        .to_string_lossy()
        .replace('\\', "/");
    Ok((found.clone(), short))
}

/// The nearest directory at or above `dir` whose `Cargo.toml` declares a
/// `[workspace]`.
fn workspace_root(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    dir.ancestors()
        .find(|d| {
            std::fs::read_to_string(d.join("Cargo.toml"))
                .is_ok_and(|toml| toml.lines().any(|line| line.trim() == "[workspace]"))
        })
        .map(std::path::Path::to_path_buf)
}

// Recursive include resolver to inline included templates at compile time at string level.
// Appends to `out` and records in `map` which file each chunk of `out` came from.
fn resolve_includes_text(
    file: usize,
    cargo_manifest_dir: &std::path::Path,
    parent_dir: &std::path::Path,
    tracked_paths: &mut Vec<String>,
    out: &mut String,
    map: &mut SourceMap,
) {
    let content = map.files[file].1.clone();
    let mut pos = 0;
    while let Some(rel_idx) = content[pos..].find("{% include") {
        let start_idx = pos + rel_idx;
        map.segments.push(Segment { start: out.len(), file, file_offset: pos });
        out.push_str(&content[pos..start_idx]);
        let rest = &content[start_idx..];
        let Some(end_idx) = rest.find("%}") else {
            fail(format_args!("unterminated `{{% include %}}`\n  --> {}", map.describe_file_offset(file, start_idx)));
        };
        let path_str = rest["{% include".len()..end_idx].trim().trim_matches(|c| c == '"' || c == '\'');
        if path_str.is_empty() {
            fail(format_args!("expected `{{% include \"path\" %}}`\n  --> {}", map.describe_file_offset(file, start_idx)));
        }

        let (full_path, short_path) = find_template_file(path_str, cargo_manifest_dir, Some(parent_dir))
            .unwrap_or_else(|e| fail(format_args!("{e}\n  --> {}", map.describe_file_offset(file, start_idx))));
        let included_content = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| fail(format_args!("failed to read included template {full_path:?}: {e}")));
        let included = map.add_file(short_path, included_content);
        tracked_paths.push(full_path.to_string_lossy().into_owned());

        let file_parent = full_path.parent().unwrap_or(parent_dir);
        resolve_includes_text(included, cargo_manifest_dir, file_parent, tracked_paths, out, map);
        pos = start_idx + end_idx + 2;
    }
    map.segments.push(Segment { start: out.len(), file, file_offset: pos });
    out.push_str(&content[pos..]);
}

/// Message for panics that are not `TemplateError`s, i.e. compiler bugs.
fn internal_error_message(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");
    format!("internal error in the xerune template compiler (please report this, with the template): {detail}")
}

#[proc_macro_derive(XeruneTemplate, attributes(template))]
pub fn derive_xerune_template(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Find the template from attributes: a file (`path`) or inline (`source`)
    let mut template = None;
    for attr in &input.attrs {
        if attr.path().is_ident("template") {
            let parsed = attr.parse_nested_meta(|meta| {
                let kind = if meta.path.is_ident("path") {
                    TemplateRef::Path
                } else if meta.path.is_ident("source") {
                    TemplateRef::Source
                } else {
                    return Err(meta.error("unsupported attribute; expected `path = \"...\"` or `source = \"...\"`"));
                };
                let lit: syn::LitStr = meta.value()?.parse()?;
                if template.is_some() {
                    return Err(meta.error("only one of `path` / `source` may be given"));
                }
                template = Some((kind(lit.value()), lit.span()));
                Ok(())
            });
            if let Err(e) = parsed {
                return e.to_compile_error().into();
            }
        }
    }
    let Some((template, template_span)) = template else {
        return syn::Error::new_spanned(&input.ident, "XeruneTemplate requires a `#[template(path = \"...\")]` attribute")
            .to_compile_error()
            .into();
    };

    // Template errors unwind out of the (deeply recursive) compiler as
    // `TemplateError` panics; report them as ordinary compile errors pointing
    // at the `path = "..."` literal. Silence the default hook meanwhile so the
    // build output isn't cluttered with "thread panicked" noise.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| expand_template(&input, &template)));
    std::panic::set_hook(prev_hook);
    CURRENT_TEMPLATE.with(|cur| cur.borrow_mut().take());
    TEMPLATE_OWNERS.with(|m| m.borrow_mut().clear());

    match result {
        Ok(tokens) => tokens.into(),
        Err(payload) => {
            let msg = match payload.downcast::<TemplateError>() {
                Ok(err) => err.0,
                Err(other) => internal_error_message(&*other),
            };
            syn::Error::new(template_span, msg).to_compile_error().into()
        }
    }
}

/// `#[template(path = "...")]` or `#[template(source = "...")]`.
enum TemplateRef {
    Path(String),
    Source(String),
}

fn expand_template(input: &DeriveInput, template: &TemplateRef) -> proc_macro2::TokenStream {
    let name = &input.ident;

    let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cargo_manifest_path = PathBuf::from(&cargo_manifest_dir);

    // Load the template; includes in an inline template resolve from the manifest dir
    let mut source_map = SourceMap::default();
    let mut tracked_paths = Vec::new();
    let (root_file, parent_dir) = match template {
        TemplateRef::Path(template_path) => {
            let (full_template_path, short_path) = find_template_file(template_path, &cargo_manifest_path, None)
                .unwrap_or_else(|e| fail(e));
            let template_content = std::fs::read_to_string(&full_template_path)
                .unwrap_or_else(|e| fail(format_args!("failed to read template file {full_template_path:?}: {e}")));
            let root_file = source_map.add_file(short_path, template_content);
            tracked_paths.push(full_template_path.to_string_lossy().into_owned());
            (root_file, full_template_path.parent().unwrap_or(&cargo_manifest_path).to_path_buf())
        }
        TemplateRef::Source(source) => {
            (source_map.add_file("<inline template>".to_string(), source.clone()), cargo_manifest_path.clone())
        }
    };
    let parent_dir = parent_dir.as_path();

    // Resolve includes at string level, remembering where each chunk came from
    let mut resolved_content = String::new();
    resolve_includes_text(root_file, &cargo_manifest_path, parent_dir, &mut tracked_paths, &mut resolved_content, &mut source_map);
    let template_content_ref: &'static str = Box::leak(resolved_content.into_boxed_str());
    CURRENT_TEMPLATE.with(|cur| *cur.borrow_mut() = Some((template_content_ref, source_map)));

    // Rebuild when a template changes: cargo tracks files read by include_bytes.
    tracked_paths.sort();
    tracked_paths.dedup();
    let dummy_includes = tracked_paths.iter().map(|path| {
        quote! {
            const _: &[u8] = include_bytes!(#path);
        }
    });

    // Parse template using askama_parser
    let syntax = Syntax::default();
    let ast = Ast::from_str(template_content_ref, None, &syntax).unwrap_or_else(|e| {
        let msg = e.message.as_deref().unwrap_or("could not parse");
        let loc = CURRENT_TEMPLATE.with(|cur| cur.borrow().as_ref().map(|(_, map)| map.describe(e.offset)));
        let note = if msg.starts_with("unknown node") {
            "\n  = note: conditions use Rust operators: `&&`, `||`, `!` (not `and`, `or`, `not`)"
        } else {
            ""
        };
        fail(format_args!("template syntax error: {msg}\n  --> {}{note}", loc.unwrap_or_default()))
    });
    validate_nodes(ast.nodes());

    let mut dynamic_counter = 0;
    let mut dynamic_exprs = HashMap::new();
    let mut dynamic_loops = HashMap::new();
    let mut dynamic_ifs = HashMap::new();
    
    let mut markup = Markup::Text;
    let mut in_quote = None;
    let preprocessed_html = preprocess_nodes(
        ast.nodes(),
        &mut dynamic_counter,
        &mut dynamic_exprs,
        &mut dynamic_loops,
        &mut dynamic_ifs,
        &mut markup,
        &mut in_quote,
    );

    // Extract CSS stylesheet from the template
    let css_content = extract_css_from_html(&preprocessed_html);

    // Parse the preprocessed DOM tree
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut preprocessed_html.as_bytes())
        .unwrap();
    record_template_owners(&dom.document);

    // The stylesheet parser skips at-rules, so split `@media` blocks out first:
    // each region keeps its parsed rules plus a runtime viewport guard.
    let media_pieces = split_media_css(&css_content);
    let mut rulesets: Vec<MediaRuleSet> = Vec::new();
    let mut keyframes_css = String::new();
    for (query, text) in &media_pieces {
        keyframes_css.push_str(text);
        keyframes_css.push('\n');
        let sheet = xerune_css::sheet::StyleSheet::parse(text);
        let guard = match query {
            None => None,
            Some(q) => {
                let (guard, unsupported) = compile_media_guard(q);
                if unsupported {
                    eprintln!("xerune: @media query '{}' uses unsupported/unknown features; those parts never apply", q);
                }
                match guard {
                    MediaGuard::Always => None,
                    MediaGuard::Guard(t) => Some(t),
                }
            }
        };
        rulesets.push(MediaRuleSet { sheet, guard });
    }

    // Collect data-on-click action strings
    let mut raw_actions = Vec::new();
    fn collect_actions(handle: &Handle, actions: &mut Vec<String>) {
        if let NodeData::Element { ref attrs, .. } = handle.data {
            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == "data-on-click" {
                    actions.push(attr.value.to_string());
                }
            }
        }
        for child in handle.children.borrow().iter() {
            collect_actions(child, actions);
        }
    }
    collect_actions(&dom.document, &mut raw_actions);

    let mut check_calls = Vec::new();
    for action in raw_actions {
        if let Some(idx) = action.find("{{").or_else(|| action.find("{%")) {
            let prefix = &action[..idx];
            if !prefix.is_empty() {
                let panic_msg = format!("Invalid action prefix '{}' in HTML template", prefix);
                check_calls.push(quote! {
                    {
                        let mut found = false;
                        let mut i = 0;
                        while i < prefixes.len() {
                            if const_str_starts_with(#prefix, prefixes[i]) {
                                found = true;
                                break;
                            }
                            i += 1;
                        }
                        if !found {
                            panic!(#panic_msg);
                        }
                    }
                });
            }
        } else {
            let panic_msg = format!("Invalid action '{}' in HTML template", action);
            check_calls.push(quote! {
                {
                    let mut found = false;
                    let mut i = 0;
                    while i < exact.len() {
                        if const_str_eq(#action, exact[i]) {
                            found = true;
                            break;
                        }
                        i += 1;
                    }
                    if !found {
                        i = 0;
                        while i < prefixes.len() {
                            if const_str_starts_with(#action, prefixes[i]) {
                                found = true;
                                break;
                            }
                            i += 1;
                        }
                    }
                    if !found {
                        panic!(#panic_msg);
                    }
                }
            });
        }
    }

    // Only the @keyframes blocks are parsed at runtime (on every rebuild),
    // so hand over just those, and nothing for templates without any.
    let keyframes_css = extract_keyframes(&keyframes_css);
    let keyframes_init = if keyframes_css.is_empty() {
        quote! {}
    } else {
        quote! { builder.set_keyframes(::xerune::__private::parse_keyframes(#keyframes_css)); }
    };

    let mut local_vars = HashSet::new();
    let body_compilation = compile_dom_node(&dom.document, &rulesets, &mut local_vars, &dynamic_exprs, &dynamic_loops, &dynamic_ifs);

    let expanded = quote! {
        #(#dummy_includes)*

        impl ::xerune::__private::TemplateLayout for #name {
            fn stylesheet(&self) -> &'static str {
                ""
            }

            fn build_ui(
                &self,
                builder: &mut ::xerune::__private::UiBuilder,
                measurer: &impl ::xerune::TextMeasurer,
                default_style: &::xerune::__private::ContainerStyle,
                message_validator: &impl Fn(&str) -> bool,
            ) -> ::xerune::__private::NodeId {
                const _: () = {
                    const fn const_str_eq(a: &str, b: &str) -> bool {
                        let a_bytes = a.as_bytes();
                        let b_bytes = b.as_bytes();
                        if a_bytes.len() != b_bytes.len() { return false; }
                        let mut i = 0;
                        while i < a_bytes.len() {
                            if a_bytes[i] != b_bytes[i] { return false; }
                            i += 1;
                        }
                        true
                    }
                    const fn const_str_starts_with(s: &str, prefix: &str) -> bool {
                        let s_bytes = s.as_bytes();
                        let p_bytes = prefix.as_bytes();
                        if s_bytes.len() < p_bytes.len() { return false; }
                        let mut i = 0;
                        while i < p_bytes.len() {
                            if s_bytes[i] != p_bytes[i] { return false; }
                            i += 1;
                        }
                        true
                    }

                    type MsgType = <#name as ::xerune::Model>::Message;
                    let exact = <MsgType as ::xerune::XeruneMessage>::VALID_EXACT;
                    let prefixes = <MsgType as ::xerune::XeruneMessage>::VALID_PREFIXES;

                    #(#check_calls)*
                };

                #keyframes_init
                let parent = builder.begin_root(default_style);
                let parent_style = default_style.clone();
                {
                    let parent = parent;
                    let parent_style = parent_style;
                    #body_compilation
                }
                parent
            }
        }
    };

    expanded
}

/// The hidden constructor `#[derive(XeruneMessage)]` generates for a message
/// name (`add`) or prefix (`toggle:`), which compiled templates call. `_` is
/// doubled and other non-alphanumerics are hex-escaped, so distinct messages
/// never share a name.
fn message_constructor(message: &str) -> syn::Ident {
    let mut name = String::from("__xerune_msg_");
    for c in message.chars() {
        match c {
            c if c.is_ascii_alphanumeric() => name.push(c),
            '_' => name.push_str("__"),
            c => name.push_str(&format!("_{:x}_", c as u32)),
        }
    }
    syn::Ident::new(&name, proc_macro2::Span::call_site())
}

#[proc_macro_derive(XeruneMessage, attributes(xerune))]
pub fn derive_xerune_message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let mut no_from_str = false;
    for attr in &input.attrs {
        if attr.path().is_ident("xerune") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("no_from_str") {
                    no_from_str = true;
                    Ok(())
                } else {
                    Err(meta.error("unsupported attribute"))
                }
            });
        }
    }

    let data = match &input.data {
        syn::Data::Enum(d) => d,
        _ => panic!("XeruneMessage can only be derived on enums"),
    };

    let mut exact_matches = Vec::new();
    let mut prefix_matches = Vec::new();
    let mut from_str_branches = Vec::new();
    let mut constructors = Vec::new();

    for variant in &data.variants {
        let variant_name = &variant.ident;

        let to_snake_case = |s: &str| {
            let mut res = String::new();
            for (i, c) in s.chars().enumerate() {
                if c.is_uppercase() {
                    if i > 0 {
                        res.push('_');
                    }
                    res.push(c.to_ascii_lowercase());
                } else {
                    res.push(c);
                }
            }
            res
        };

        let mut custom_rename = None;
        let mut custom_prefix = None;
        for attr in &variant.attrs {
            if attr.path().is_ident("xerune") {
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        custom_rename = Some(s.value());
                        Ok(())
                    } else if meta.path.is_ident("prefix") {
                        let value = meta.value()?;
                        let s: syn::LitStr = value.parse()?;
                        custom_prefix = Some(s.value());
                        Ok(())
                    } else {
                        Err(meta.error("unsupported attribute"))
                    }
                });
            }
        }

        match &variant.fields {
            syn::Fields::Unit => {
                let exact_str = custom_rename.unwrap_or_else(|| to_snake_case(&variant_name.to_string()));
                exact_matches.push(exact_str.clone());
                from_str_branches.push(quote! {
                    #exact_str => Ok(#name::#variant_name),
                });
                let constructor = message_constructor(&exact_str);
                constructors.push(quote! {
                    pub fn #constructor() -> Self {
                        #name::#variant_name
                    }
                });
            }
            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = &fields.unnamed[0];
                let field_type = &field.ty;

                let prefix_str = custom_prefix.unwrap_or_else(|| {
                    let mut s = to_snake_case(&variant_name.to_string());
                    s.push(':');
                    s
                });
                prefix_matches.push(prefix_str.clone());
                let constructor = message_constructor(&prefix_str);
                constructors.push(quote! {
                    pub fn #constructor(value: #field_type) -> Self {
                        #name::#variant_name(value)
                    }
                });

                from_str_branches.push(quote! {
                    _s if _s.starts_with(#prefix_str) => {
                        let payload = &_s[#prefix_str.len()..];
                        if let Ok(val) = payload.parse::<#field_type>() {
                            Ok(#name::#variant_name(val))
                        } else {
                            Err(())
                        }
                    }
                });
            }
            _ => panic!("XeruneMessage only supports unit variants and single-field tuple variants currently. For custom variants, please use manual implementation with #[xerune(no_from_str)]."),
        }
    }

    let from_str_impl = if no_from_str {
        quote! {}
    } else {
        quote! {
            impl core::str::FromStr for #name {
                type Err = ();
                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    match s {
                        #(#from_str_branches)*
                        _ => Err(()),
                    }
                }
            }
        }
    };

    let expanded = quote! {
        #from_str_impl

        /// Constructors for compiled `data-on-click` messages.
        #[doc(hidden)]
        #[allow(non_snake_case)]
        impl #name {
            #(#constructors)*
        }

        impl ::xerune::XeruneMessage for #name {
            const VALID_PREFIXES: &'static [&'static str] = &[#(#prefix_matches),*];
            const VALID_EXACT: &'static [&'static str] = &[#(#exact_matches),*];
        }
    };

    TokenStream::from(expanded)
}

