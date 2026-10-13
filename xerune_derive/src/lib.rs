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
        expr_str
    }
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
        None => expr_str,
    }
}

// Generate HTML with placeholders
fn preprocess_nodes<'a>(
    nodes: &'a [Node<'a>],
    dynamic_counter: &mut usize,
    dynamic_exprs: &mut HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &mut HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &mut HashMap<usize, &'a Node<'a>>,
    in_tag: &mut bool,
    in_quote: &mut Option<char>,
) -> String {
    let mut html = String::new();
    for node in nodes {
        match node {
            Node::Lit(lit) => {
                let val = format!("{}{}{}", lit.lws, lit.val, lit.rws);
                for c in val.chars() {
                    if *in_tag {
                        if let Some(quote_char) = *in_quote {
                            if c == quote_char {
                                *in_quote = None;
                            }
                        } else if c == '"' || c == '\'' {
                            *in_quote = Some(c);
                        } else if c == '>' {
                            *in_tag = false;
                        }
                    } else if c == '<' {
                        *in_tag = true;
                    }
                }
                html.push_str(&val);
            }
            Node::Comment(_) => {}
            Node::Expr(_, expr) => {
                if *in_tag {
                    push_tag_level(&mut html, node, *in_quote, "template expressions");
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_exprs.insert(id, &**expr);
                    html.push_str(&format!("<template expr-id=\"{}\"></template>", id));
                }
            }
            Node::Loop(loop_box) => {
                if *in_tag {
                    push_tag_level(&mut html, node, *in_quote, "`{% for %}`");
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_loops.insert(id, node);
                    html.push_str(&format!("<template loop-id=\"{}\">", id));
                    html.push_str(&preprocess_nodes(&loop_box.body, dynamic_counter, dynamic_exprs, dynamic_loops, dynamic_ifs, in_tag, in_quote));
                    html.push_str("</template>");
                }
            }
            Node::If(if_node) => {
                if *in_tag {
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
                        html.push_str(&preprocess_nodes(&branch.nodes, dynamic_counter, dynamic_exprs, dynamic_loops, dynamic_ifs, in_tag, in_quote));
                        html.push_str("</template>");
                    }
                    html.push_str("</template>");
                }
            }
            Node::Break(_) => {
                if *in_tag {
                    push_tag_level(&mut html, node, *in_quote, "`{% break %}`");
                } else {
                    html.push_str("<template break></template>");
                }
            }
            Node::Continue(_) => {
                if *in_tag {
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
}

fn get_static_classes(handle: &Handle) -> HashSet<String> {
    let mut static_classes = HashSet::new();
    if let NodeData::Element { ref attrs, .. } = handle.data {
        if let Some(attr) = attrs.borrow().iter().find(|a| a.name.local.as_ref() == "class") {
            let class_val = attr.value.as_ref();
            let parts = class_val.split_whitespace();
            for part in parts {
                if !part.contains("{%") && !part.contains("{{") && !part.contains("%}") && !part.contains("}}") {
                    static_classes.insert(part.to_string());
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

impl simplecss::Element for MacroElementWrapper {
    // The nearest real element, skipping the `<template>` wrappers that stand
    // in for `{% for %}`/`{% if %}`. Sibling-based selectors (`+`,
    // `:first-child`) still see the wrappers and so never match across them:
    // which iteration/branch comes first is only known at runtime.
    fn parent_element(&self) -> Option<Self> {
        let mut parent = dom_parent(&self.handle)?;
        while is_template_element(&parent) {
            parent = dom_parent(&parent)?;
        }
        Some(MacroElementWrapper {
            active_classes: get_static_classes(&parent),
            handle: parent,
        })
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let parent = self.parent_element()?;
        let siblings = parent.handle.children.borrow();
        let index = siblings.iter().position(|child| std::rc::Rc::ptr_eq(child, &self.handle))?;
        if index > 0 {
            for i in (0..index).rev() {
                let sibling = &siblings[i];
                if matches!(sibling.data, NodeData::Element { .. }) {
                    return Some(MacroElementWrapper {
                        active_classes: get_static_classes(sibling),
                        handle: sibling.clone(),
                    });
                }
            }
        }
        None
    }

    fn has_local_name(&self, name: &str) -> bool {
        if let NodeData::Element { name: ref element_name, .. } = self.handle.data {
            element_name.local.as_ref() == name
        } else {
            false
        }
    }

    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        if local_name == "class" {
            for cls in &self.active_classes {
                if operator.matches(cls) {
                    return true;
                }
            }
        }
        if let NodeData::Element { ref attrs, .. } = self.handle.data {
            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == local_name {
                    return operator.matches(attr.value.as_ref());
                }
            }
        }
        false
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => {
                if let Some(parent) = self.parent_element() {
                    let siblings = parent.handle.children.borrow();
                    for child in siblings.iter() {
                        if matches!(child.data, NodeData::Element { .. }) {
                            return std::rc::Rc::ptr_eq(child, &self.handle);
                        }
                    }
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }
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
    declarations: Vec<(String, String)>,
    condition: Option<String>,
    /// Guard expression (`xerune::css::media::matches_query(...)`) when the
    /// rule was declared inside an `@media` block; None = always applies.
    media_guard: Option<proc_macro2::TokenStream>,
}

/// A parsed stylesheet region together with the `@media` guard it appeared under.
struct MediaRuleSet {
    sheet: simplecss::StyleSheet<'static>,
    /// Some(expr) => wrap rule application in `if expr`; None => always.
    guard: Option<proc_macro2::TokenStream>,
}

/// Splits stylesheet text into `(query, css)` regions; `query == None` marks
/// top-level rules. Mirrors `xerune::css::media::split_segments` semantics
/// (kept as a standalone copy because proc-macro crates cannot depend on
/// the crate they generate code for). Nested `@media` blocks are flattened
/// by AND-ing their queries.
fn split_media_css(css: &str) -> Vec<(Option<String>, String)> {
    fn find_matching_brace(s: &str, open: usize) -> Option<usize> {
        let bytes = s.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn is_media_at(s: &str, i: usize) -> bool {
        let bytes = s.as_bytes();
        const NAME: &[u8] = b"media";
        if i + 1 + NAME.len() > bytes.len() {
            return false;
        }
        if !bytes[i + 1..i + 1 + NAME.len()].eq_ignore_ascii_case(NAME) {
            return false;
        }
        match bytes.get(i + 1 + NAME.len()) {
            None => true,
            Some(&c) => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'('),
        }
    }

    let mut out = Vec::new();
    let bytes = css.as_bytes();
    let mut base_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let end = css[i + 2..].find("*/").map(|e| i + 2 + e + 2).unwrap_or(bytes.len());
            i = end;
            continue;
        }
        if bytes[i] == b'@' && is_media_at(css, i) {
            if i > base_start && !css[base_start..i].trim().is_empty() {
                out.push((None, css[base_start..i].to_string()));
            }
            let open = match css[i..].find('{') {
                Some(o) => i + o,
                None => break,
            };
            let query = css[i + 6..open].trim().to_string();
            let close = match find_matching_brace(css, open) {
                Some(c) => c,
                None => break,
            };
            for (sub_query, sub_css) in split_media_css(&css[open + 1..close]) {
                match sub_query {
                    None => out.push((Some(query.clone()), sub_css)),
                    Some(sq) => out.push((Some(format!("{} and {}", query, sq)), sub_css)),
                }
            }
            i = close + 1;
            base_start = i;
            continue;
        }
        i += 1;
    }
    if base_start < css.len() && !css[base_start..].trim().is_empty() {
        out.push((None, css[base_start..].to_string()));
    }
    out
}

/// Compile-time representation mirroring `xerune::css::media::Feature`.
enum MediaFeature {
    MinWidth(f32),
    MaxWidth(f32),
    MinHeight(f32),
    MaxHeight(f32),
    Landscape,
    Portrait,
}

impl MediaFeature {
    fn tokens(&self) -> proc_macro2::TokenStream {
        match self {
            MediaFeature::MinWidth(v) => quote! { ::xerune::css::media::Feature::MinWidth(#v) },
            MediaFeature::MaxWidth(v) => quote! { ::xerune::css::media::Feature::MaxWidth(#v) },
            MediaFeature::MinHeight(v) => quote! { ::xerune::css::media::Feature::MinHeight(#v) },
            MediaFeature::MaxHeight(v) => quote! { ::xerune::css::media::Feature::MaxHeight(#v) },
            MediaFeature::Landscape => quote! { ::xerune::css::media::Feature::Landscape },
            MediaFeature::Portrait => quote! { ::xerune::css::media::Feature::Portrait },
        }
    }
}

fn parse_media_value(val: &str) -> Option<f32> {
    if let Some(stripped) = val.strip_suffix("px") {
        stripped.trim().parse::<f32>().ok()
    } else if val.ends_with('%') || val.ends_with("em") || val.ends_with("rem") {
        None
    } else {
        val.parse::<f32>().ok()
    }
}

fn parse_media_feature(inner: &str) -> Option<MediaFeature> {
    let (name, value) = inner.split_once(':')?;
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim();
    match name.as_str() {
        "orientation" => match value.to_ascii_lowercase().as_str() {
            "landscape" => Some(MediaFeature::Landscape),
            "portrait" => Some(MediaFeature::Portrait),
            _ => None,
        },
        "min-width" | "device-min-width" => Some(MediaFeature::MinWidth(parse_media_value(value)?)),
        "max-width" | "device-max-width" => Some(MediaFeature::MaxWidth(parse_media_value(value)?)),
        "min-height" | "device-min-height" => Some(MediaFeature::MinHeight(parse_media_value(value)?)),
        "max-height" | "device-max-height" => Some(MediaFeature::MaxHeight(parse_media_value(value)?)),
        _ => None,
    }
}

/// Result of compiling an `@media` query text to a runtime guard.
enum MediaGuard {
    /// Query always matches (`all`, `screen`, or empty) — no guard emitted.
    Always,
    /// Query can never match (malformed, unsupported feature, `print`).
    Never,
    /// Positive query compiled to a `matches_query` call.
    Guard(proc_macro2::TokenStream),
}

/// Compiles an `@media` query text into a runtime guard expression that
/// evaluates against `xerune::screen`'s global viewport at build time.
/// Grammar mirrors `xerune::css::media::parse_query`.
fn compile_media_guard(query: &str) -> MediaGuard {
    let text = query.trim();
    if text.is_empty() {
        return MediaGuard::Always;
    }

    let mut group_tokens = Vec::new();
    let mut any_effective = false;

    for part in split_comma_top_level(text) {
        let part = part.trim().to_string();
        if part.is_empty() {
            continue;
        }

        // `not` prefix.
        let (negated, body) = match part.strip_prefix("not") {
            Some(rest) if rest.starts_with(' ') || rest.starts_with('(') || rest.starts_with('\t') => (true, rest.trim_start()),
            _ => (false, part.as_str()),
        };

        let mut features: Vec<MediaFeature> = Vec::new();
        let mut unsatisfiable = false;

        for cond in split_and_top_level(body) {
            let cond = cond.trim().to_string();
            if cond.is_empty() {
                unsatisfiable = true;
                break;
            }
            if cond.starts_with('(') && cond.ends_with(')') {
                match parse_media_feature(&cond[1..cond.len() - 1]) {
                    Some(f) => features.push(f),
                    None => {
                        // Unknown feature per spec: never matches.
                        unsatisfiable = true;
                        break;
                    }
                }
            } else {
                let lower = cond.to_ascii_lowercase();
                match lower.as_str() {
                    "all" | "screen" => {}
                    "print" => {
                        unsatisfiable = true;
                        break;
                    }
                    _ => {
                        unsatisfiable = true;
                        break;
                    }
                }
            }
        }

        if unsatisfiable {
            // Emit an impossible group; other comma-groups may still match.
            group_tokens.push(quote! {
                ::xerune::css::media::FeatureGroup { negated: false, features: &[::xerune::css::media::Feature::MinWidth(f32::INFINITY)] }
            });
            any_effective = true;
            continue;
        }

        if negated {
            // Negation of a positive feature set: express via matches_query on
            // the complement is not directly representable with OR-of-ANDs.
            // Only support the single common form `not (feature)` by emitting
            // a negated group token.
            let feats = features.iter().map(|f| f.tokens());
            group_tokens.push(quote! {
                ::xerune::css::media::FeatureGroup { negated: true, features: &[#(#feats),*] }
            });
            any_effective = true;
            continue;
        }

        if features.is_empty() {
            // Media-type-only group: always matches, so the whole query is
            // unguarded regardless of the other groups.
            return MediaGuard::Always;
        }

        let feats = features.iter().map(|f| f.tokens());
        group_tokens.push(quote! {
            ::xerune::css::media::FeatureGroup { negated: false, features: &[#(#feats),*] }
        });
        any_effective = true;
    }

    if !any_effective {
        return MediaGuard::Never;
    }

    MediaGuard::Guard(quote! {
        ::xerune::css::media::matches_query(&[#(#group_tokens),*])
    })
}

/// Splits on commas outside parentheses.
fn split_comma_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Splits on top-level ` and ` (case-insensitive, whitespace-delimited).
fn split_and_top_level(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'a' | b'A' if depth == 0 => {
                if i + 4 <= bytes.len()
                    && bytes[i..i + 4].eq_ignore_ascii_case(b"and ")
                    && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r'))
                {
                    parts.push(&s[start..i]);
                    i += 4;
                    start = i;
                    continue;
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
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
            let mut children_code = Vec::new();
            for child in get_node_children(handle).iter() {
                children_code.push(compile_dom_node(child, rulesets, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
            }
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
                        let node = {
                            use ::xerune::ui::ToDisplayString;
                            let text_val = #expr_tokens.to_display_string().into_owned();
                            let mut current_style = parent_style.clone();
                            current_style.background_color = None;
                            current_style.background_gradient = None;
                            current_style.box_shadow = None;
                            current_style.border_width = 0.0;
                            current_style.border_radius = 0.0;
                            current_style.border_color = None;
                            current_style.overflow = ::xerune::Overflow::Visible;
                            current_style.animation_name = None;
                            current_style.animation_duration = 0.0;
                            current_style.animation_timing_function = std::sync::Arc::from("ease");
                            current_style.animation_delay = 0.0;
                            current_style.animation_iteration_count = ::xerune::style::AnimationIterationCount::Count(1.0);
                            current_style.animation_direction = std::sync::Arc::from("normal");
                            current_style.animation_fill_mode = std::sync::Arc::from("none");
                            current_style.animation_play_state = std::sync::Arc::from("running");

                            let normalized = ::xerune::ui::normalize_text(&text_val);
                            let (width, height) = measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                            let text_layout_style = ::xerune::taffy::style::Style {
                                size: ::xerune::taffy::geometry::Size { 
                                    width: ::xerune::taffy::style::Dimension::length(width), 
                                    height: ::xerune::taffy::style::Dimension::length(height) 
                                },
                                ..::xerune::taffy::style::Style::default()
                            };
                            let node_id = builder.taffy.new_leaf(text_layout_style.clone()).unwrap();
                            builder.render_data.insert(node_id, ::xerune::style::RenderData::Text(normalized.into_owned(), current_style.clone()));
                            builder.base_styles.insert(node_id, (text_layout_style, current_style));
                            node_id
                        };
                        builder.append_child(parent, node);
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
                        
                        let mut children_code = Vec::new();
                        for child in get_node_children(handle).iter() {
                            children_code.push(compile_dom_node(child, rulesets, &mut loop_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
                        }
                        
                        let var_tokens: proc_macro2::TokenStream = var_str.parse().unwrap();
                        let iter_tokens: proc_macro2::TokenStream = iter_str.parse().unwrap();

                        return quote! {
                            {
                                let parent = parent;
                                let parent_style = parent_style.clone();
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
                                let mut children_code = Vec::new();
                                for child in get_node_children(&bh).iter() {
                                    children_code.push(compile_dom_node(child, rulesets, &mut branch_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
                                }
                                
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

            let mut matched_rules = Vec::new();
            for rs in rulesets {
                for rule in &rs.sheet.rules {
                    let mut wrapper = MacroElementWrapper {
                        handle: handle.clone(),
                        active_classes: static_classes.clone(),
                    };

                    if rule.selector.matches(&wrapper) {
                        matched_rules.push(MatchedRule {
                            declarations: rule.declarations.iter().map(|d| (d.name.to_string(), d.value.to_string())).collect(),
                            condition: None,
                            media_guard: rs.guard.clone(),
                        });
                    } else {
                        for (cls, cond) in &conditional_classes {
                            let mut active = static_classes.clone();
                            active.insert(cls.clone());
                            wrapper.active_classes = active;
                            if rule.selector.matches(&wrapper) {
                                matched_rules.push(MatchedRule {
                                    declarations: rule.declarations.iter().map(|d| (d.name.to_string(), d.value.to_string())).collect(),
                                    condition: Some(cond.clone()),
                                    media_guard: rs.guard.clone(),
                                });
                            }
                        }
                    }
                }
            }

            let mut rule_applications = Vec::new();
            for rule in matched_rules {
                let mut app = Vec::new();
                for (prop_name, prop_val) in rule.declarations {
                    app.push(quote! {
                        ::xerune::css::apply_declaration(#prop_name, #prop_val, &mut current_style, &mut layout_style);
                    });
                }
                let mut body: proc_macro2::TokenStream = quote! { #(#app)* };
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
            for (idx, attr) in attrs.borrow().iter().enumerate() {
                let key = attr.name.local.as_ref();
                let val = attr.value.as_ref();
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

            let mut child_compilation = Vec::new();
            for child in get_node_children(handle).iter() {
                child_compilation.push(compile_dom_node(child, rulesets, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
            }

            quote! {
                let (node, mut layout_style, mut current_style, parsed) = {
                    #(#dynamic_vars)*
                    let attrs_slice: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'_, str>)] = &[
                        #(#static_attrs,)*
                        #(#dynamic_attrs),*
                    ];

                    let defaults = ::xerune::defaults::get_default_style(#tag, &parent_style);
                    let mut layout_style = defaults.taffy_style;
                    let mut current_style = defaults.container_style;

                    // Match and apply CSS rules
                    #(#rule_applications)*

                    // Match and apply inline/dynamic attributes
                    let mut parsed = ::xerune::ui::attributes::ParsedAttributes::new(defaults.element_type);
                    ::xerune::ui::attributes::parse_attributes_generic(
                        #tag,
                        attrs_slice.iter().map(|(k, v)| (k.as_ref(), v.as_ref())),
                        &mut current_style,
                        &mut layout_style,
                        &mut parsed,
                        message_validator,
                    );

                    // Create the taffy node
                    let node_id = builder.taffy.new_leaf(layout_style.clone()).unwrap();

                    (node_id, layout_style, current_style, parsed)
                };
                builder.append_child(parent, node);
                {
                    let parent = node;
                    let parent_style = current_style.clone();
                    #(#child_compilation)*
                }
                
                // Finalize style and insert into Maps after children are appended
                ::xerune::ui::style_resolution::finalize_node_style(
                    node,
                    #tag,
                    &parent_style,
                    &mut layout_style,
                    &mut current_style,
                    &parsed,
                    &mut builder.taffy,
                    &mut builder.render_data,
                    &mut builder.interactions,
                    &mut builder.base_styles,
                );
            }
        }
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return quote! {};
            }
            quote! {
                let node = {
                    let mut current_style = parent_style.clone();
                    current_style.background_color = None;
                    current_style.background_gradient = None;
                    current_style.box_shadow = None;
                    current_style.border_width = 0.0;
                    current_style.border_radius = 0.0;
                    current_style.border_color = None;
                    current_style.overflow = ::xerune::Overflow::Visible;
                    current_style.animation_name = None;
                    current_style.animation_duration = 0.0;
                    current_style.animation_timing_function = std::sync::Arc::from("ease");
                    current_style.animation_delay = 0.0;
                    current_style.animation_iteration_count = ::xerune::style::AnimationIterationCount::Count(1.0);
                    current_style.animation_direction = std::sync::Arc::from("normal");
                    current_style.animation_fill_mode = std::sync::Arc::from("none");
                    current_style.animation_play_state = std::sync::Arc::from("running");

                    let normalized = ::xerune::ui::normalize_text(#trimmed);
                    let (width, height) = measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                    let text_layout_style = ::xerune::taffy::style::Style {
                        size: ::xerune::taffy::geometry::Size { 
                            width: ::xerune::taffy::style::Dimension::length(width), 
                            height: ::xerune::taffy::style::Dimension::length(height) 
                        },
                        ..::xerune::taffy::style::Style::default()
                    };
                    let node_id = builder.taffy.new_leaf(text_layout_style.clone()).unwrap();
                    builder.render_data.insert(node_id, ::xerune::style::RenderData::Text(normalized.into_owned(), current_style.clone()));
                    builder.base_styles.insert(node_id, (text_layout_style, current_style));
                    node_id
                };
                builder.append_child(parent, node);
            }
        }
        _ => quote! {},
    }
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
fn find_template_file(
    path_str: &str,
    cargo_manifest_dir: &std::path::Path,
    parent_dir: Option<&std::path::Path>,
) -> Result<(std::path::PathBuf, String), String> {
    // 1. Check relative to parent_dir if provided
    if let Some(parent) = parent_dir {
        let candidate = parent.join(path_str);
        if candidate.is_file() {
            if let Ok(rel) = candidate.strip_prefix(cargo_manifest_dir) {
                let rel_str = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
                return Ok((candidate, rel_str));
            }
        }
    }

    // 2. Check direct path relative to cargo_manifest_dir
    let candidate = cargo_manifest_dir.join(path_str);
    if candidate.is_file() {
        let rel_str = format!("/{}", path_str.replace('\\', "/"));
        return Ok((candidate, rel_str));
    }

    // 3. Check cargo_manifest_dir/templates/<path_str>
    let candidate = cargo_manifest_dir.join("templates").join(path_str);
    if candidate.is_file() {
        let rel_str = format!("/templates/{}", path_str.replace('\\', "/"));
        return Ok((candidate, rel_str));
    }

    // 4. Search inside examples/*/templates/<path_str> (in cargo_manifest_dir and its parent workspace root)
    let search_roots = [cargo_manifest_dir, cargo_manifest_dir.parent().unwrap_or(cargo_manifest_dir)];
    for root in search_roots {
        let examples_dir = root.join("examples");
        if examples_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&examples_dir) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        let cand1 = entry_path.join("templates").join(path_str);
                        if cand1.is_file() {
                            if let Ok(rel) = cand1.strip_prefix(cargo_manifest_dir) {
                                let rel_str = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
                                return Ok((cand1, rel_str));
                            } else if let Ok(rel) = cand1.strip_prefix(root) {
                                let rel_str = format!("/../{}", rel.to_string_lossy().replace('\\', "/"));
                                return Ok((cand1, rel_str));
                            }
                        }
                        let cand2 = entry_path.join(path_str);
                        if cand2.is_file() {
                            if let Ok(rel) = cand2.strip_prefix(cargo_manifest_dir) {
                                let rel_str = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
                                return Ok((cand2, rel_str));
                            } else if let Ok(rel) = cand2.strip_prefix(root) {
                                let rel_str = format!("/../{}", rel.to_string_lossy().replace('\\', "/"));
                                return Ok((cand2, rel_str));
                            }
                        }
                    }
                }
            }
        }
    }

    Err(format!("Template file '{}' not found near manifest {:?}", path_str, cargo_manifest_dir))
}

// Recursive include resolver to inline included templates at compile time at string level.
// Appends to `out` and records in `map` which file each chunk of `out` came from.
fn resolve_includes_text(
    file: usize,
    cargo_manifest_dir: &std::path::Path,
    parent_dir: &std::path::Path,
    tracked_rel_paths: &mut Vec<String>,
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

        let (full_path, rel_path) = find_template_file(path_str, cargo_manifest_dir, Some(parent_dir))
            .unwrap_or_else(|e| fail(format_args!("{e}\n  --> {}", map.describe_file_offset(file, start_idx))));
        let included_content = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| fail(format_args!("failed to read included template {full_path:?}: {e}")));
        let included = map.add_file(rel_path.trim_start_matches('/').to_string(), included_content);
        tracked_rel_paths.push(rel_path);

        let file_parent = full_path.parent().unwrap_or(parent_dir);
        resolve_includes_text(included, cargo_manifest_dir, file_parent, tracked_rel_paths, out, map);
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
    let mut tracked_rel_paths = Vec::new();
    let (root_file, parent_dir) = match template {
        TemplateRef::Path(template_path) => {
            let (full_template_path, rel_template_path) = find_template_file(template_path, &cargo_manifest_path, None)
                .unwrap_or_else(|e| fail(e));
            let template_content = std::fs::read_to_string(&full_template_path)
                .unwrap_or_else(|e| fail(format_args!("failed to read template file {full_template_path:?}: {e}")));
            let root_file = source_map.add_file(rel_template_path.trim_start_matches('/').to_string(), template_content);
            tracked_rel_paths.push(rel_template_path);
            (root_file, full_template_path.parent().unwrap_or(&cargo_manifest_path).to_path_buf())
        }
        TemplateRef::Source(source) => {
            (source_map.add_file("<inline template>".to_string(), source.clone()), cargo_manifest_path.clone())
        }
    };
    let parent_dir = parent_dir.as_path();

    // Resolve includes at string level, remembering where each chunk came from
    let mut resolved_content = String::new();
    resolve_includes_text(root_file, &cargo_manifest_path, parent_dir, &mut tracked_rel_paths, &mut resolved_content, &mut source_map);
    let template_content_ref: &'static str = Box::leak(resolved_content.into_boxed_str());
    CURRENT_TEMPLATE.with(|cur| *cur.borrow_mut() = Some((template_content_ref, source_map)));

    tracked_rel_paths.sort();
    tracked_rel_paths.dedup();
    let dummy_includes = tracked_rel_paths.iter().map(|relative_path| {
        quote! {
            const _: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), #relative_path));
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
    
    let mut in_tag = false;
    let mut in_quote = None;
    let preprocessed_html = preprocess_nodes(
        ast.nodes(),
        &mut dynamic_counter,
        &mut dynamic_exprs,
        &mut dynamic_loops,
        &mut dynamic_ifs,
        &mut in_tag,
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

    // simplecss silently drops at-rules, so split `@media` blocks out first:
    // each region keeps its parsed rules plus a runtime viewport guard.
    let media_pieces = split_media_css(&css_content);
    let mut rulesets: Vec<MediaRuleSet> = Vec::new();
    let mut keyframes_css = String::new();
    for (query, text) in &media_pieces {
        keyframes_css.push_str(text);
        keyframes_css.push('\n');
        let sheet_text: &'static str = Box::leak(text.clone().into_boxed_str());
        let sheet = simplecss::StyleSheet::parse(sheet_text);
        let guard = match query {
            None => None,
            Some(q) => match compile_media_guard(q) {
                MediaGuard::Always => None,
                MediaGuard::Never => {
                    eprintln!("xerune: @media query '{}' uses unsupported/unknown features; its rules will never apply", q);
                    Some(quote! { false })
                }
                MediaGuard::Guard(t) => Some(t),
            },
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
        quote! { builder.keyframes = ::xerune::css::parse_keyframes(#keyframes_css); }
    };

    let mut local_vars = HashSet::new();
    let body_compilation = compile_dom_node(&dom.document, &rulesets, &mut local_vars, &dynamic_exprs, &dynamic_loops, &dynamic_ifs);

    let expanded = quote! {
        #(#dummy_includes)*

        impl ::xerune::ui::TemplateLayout for #name {
            fn stylesheet(&self) -> &'static str {
                ""
            }

            fn build_ui(
                &self,
                builder: &mut ::xerune::ui::UiBuilder,
                measurer: &impl ::xerune::TextMeasurer,
                default_style: &::xerune::style::ContainerStyle,
                message_validator: &impl Fn(&str) -> bool,
            ) -> ::xerune::taffy::NodeId {
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
                let parent_layout = ::xerune::taffy::style::Style {
                    size: ::xerune::taffy::geometry::Size {
                        width: ::xerune::taffy::style::Dimension::percent(1.0),
                        height: ::xerune::taffy::style::Dimension::percent(1.0),
                    },
                    ..::xerune::taffy::style::Style::default()
                };
                let parent = builder.taffy.new_leaf(parent_layout.clone()).unwrap();
                let parent_style = default_style.clone();
                builder.render_data.insert(parent, ::xerune::style::RenderData::Container(parent_style.clone()));
                builder.base_styles.insert(parent, (parent_layout, parent_style.clone()));
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

        impl ::xerune::model::XeruneMessage for #name {
            const VALID_PREFIXES: &'static [&'static str] = &[#(#prefix_matches),*];
            const VALID_EXACT: &'static [&'static str] = &[#(#exact_matches),*];
        }
    };

    TokenStream::from(expanded)
}

