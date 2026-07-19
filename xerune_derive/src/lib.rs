use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};
use askama_parser::{Ast, Syntax, Node, Expr, Target, node::CondTest};
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// Stringify Askama targets
fn format_target(target: &Target<'_>) -> String {
    match target {
        Target::Name(name) => name.to_string(),
        Target::Placeholder(_) => "_".to_string(),
        Target::Tuple(_, targets) => {
            let inner: Vec<String> = targets.iter().map(format_target).collect();
            format!("({})", inner.join(", "))
        }
        Target::Rest(_) => "..".to_string(),
        other => panic!("Unsupported target variant: {:?}", other),
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
        Target::Tuple(_, targets) => {
            let inner: Vec<String> = targets.iter().map(reconstruct_target).collect();
            format!("({})", inner.join(", "))
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
            format!("{} {} {}", format_expr(&**left, local_vars), op, format_expr(&**right, local_vars))
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
        format!("let {} = {}", format_target(target), expr_str)
    } else {
        expr_str
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
                    html.push_str(&reconstruct_node(node));
                } else {
                    let id = *dynamic_counter;
                    *dynamic_counter += 1;
                    dynamic_exprs.insert(id, &**expr);
                    html.push_str(&format!("<template expr-id=\"{}\"></template>", id));
                }
            }
            Node::Loop(loop_box) => {
                if *in_tag {
                    html.push_str(&reconstruct_node(node));
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
                    if in_quote.is_none() {
                        let mut attr_name = String::new();
                        let mut cond_expr = None;
                        if let Some(branch) = if_node.branches.first() {
                            if let Some(ref cond) = branch.cond {
                                cond_expr = Some(&*cond.expr);
                            }
                            if let Some(Node::Lit(lit)) = branch.nodes.first() {
                                attr_name = lit.val.trim().to_string();
                            }
                        }
                        if let (Some(expr), attr) = (cond_expr, attr_name) {
                            if attr == "checked" {
                                html.push_str(&format!(" checked=\"{{{{ {} }}}}\"", reconstruct_expr(expr)));
                            } else {
                                html.push_str(&reconstruct_node(node));
                            }
                        } else {
                            html.push_str(&reconstruct_node(node));
                        }
                    } else {
                        html.push_str(&reconstruct_node(node));
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
                    html.push_str(&reconstruct_node(node));
                } else {
                    html.push_str("<template break></template>");
                }
            }
            Node::Continue(_) => {
                if *in_tag {
                    html.push_str(&reconstruct_node(node));
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
                    let body_tokens = generate_attr_string_code(&branch.nodes, local_vars);
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
            _ => {}
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

impl simplecss::Element for MacroElementWrapper {
    fn parent_element(&self) -> Option<Self> {
        let parent_weak = self.handle.parent.take();
        let parent_opt = parent_weak.as_ref().and_then(|weak| weak.upgrade());
        self.handle.parent.set(parent_weak);
        parent_opt.map(|p| MacroElementWrapper {
            active_classes: get_static_classes(&p),
            handle: p,
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
                    for branch_with_span in &if_node.branches {
                        let branch = &**branch_with_span;
                        let branch_cond = if let Some(ref c) = branch.cond {
                            let reconstructed = format_cond_test(c, local_vars);
                            if let Some(ref parent_cond) = cond {
                                Some(format!("({}) && ({})", parent_cond, reconstructed))
                            } else {
                                Some(reconstructed)
                            }
                        } else {
                            cond.clone()
                        };
                        walk_nodes(&branch.nodes, branch_cond, local_vars, res);
                    }
                }
                _ => {}
            }
        }
    }

    let mut res = Vec::new();
    walk_nodes(ast.nodes(), None, local_vars, &mut res);
    res
}

struct MatchedRule {
    declarations: Vec<(String, String)>,
    condition: Option<String>,
}

fn compile_dom_node<'a>(
    handle: &Handle,
    stylesheet: &simplecss::StyleSheet<'_>,
    local_vars: &mut HashSet<String>,
    dynamic_exprs: &HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &HashMap<usize, &'a Node<'a>>,
) -> proc_macro2::TokenStream {
    match &handle.data {
        NodeData::Document => {
            let mut children_code = Vec::new();
            for child in get_node_children(handle).iter() {
                children_code.push(compile_dom_node(child, stylesheet, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
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
                            use xerune::ui::ToDisplayString;
                            let text_val = #expr_tokens.to_display_string().into_owned();
                            let mut current_style = parent_style.clone();
                            current_style.background_color = None;
                            current_style.background_gradient = None;
                            current_style.border_width = 0.0;
                            current_style.border_radius = 0.0;
                            current_style.border_color = None;
                            current_style.overflow = xerune::Overflow::Visible;
                            current_style.animation_name = None;
                            current_style.animation_duration = 0.0;
                            current_style.animation_timing_function = std::sync::Arc::from("ease");
                            current_style.animation_delay = 0.0;
                            current_style.animation_iteration_count = xerune::style::AnimationIterationCount::Count(1.0);
                            current_style.animation_direction = std::sync::Arc::from("normal");
                            current_style.animation_fill_mode = std::sync::Arc::from("none");
                            current_style.animation_play_state = std::sync::Arc::from("running");

                            let normalized = xerune::ui::normalize_text(&text_val);
                            let (width, height) = measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                            let text_layout_style = taffy::style::Style {
                                size: taffy::geometry::Size { 
                                    width: taffy::style::Dimension::length(width), 
                                    height: taffy::style::Dimension::length(height) 
                                },
                                ..taffy::style::Style::default()
                            };
                            let node_id = builder.taffy.new_leaf(text_layout_style.clone()).unwrap();
                            builder.render_data.insert(node_id, xerune::style::RenderData::Text(normalized.into_owned(), current_style.clone()));
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
                        let iter_str = format_expr(&loop_box.iter, local_vars);
                        
                        let mut loop_local_vars = local_vars.clone();
                        loop_local_vars.insert(var_str.clone());
                        loop_local_vars.insert("_loop_item_index".to_string());
                        loop_local_vars.insert("loop".to_string());
                        
                        let mut children_code = Vec::new();
                        for child in get_node_children(handle).iter() {
                            children_code.push(compile_dom_node(child, stylesheet, &mut loop_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
                        }
                        
                        let var_tokens: proc_macro2::TokenStream = var_str.parse().unwrap();
                        let iter_tokens: proc_macro2::TokenStream = iter_str.parse().unwrap();

                        return quote! {
                            {
                                let parent = parent;
                                let parent_style = parent_style.clone();
                                for (mut _loop_item_index, #var_tokens) in #iter_tokens.iter().enumerate() {
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
                                let mut children_code = Vec::new();
                                for child in get_node_children(&bh).iter() {
                                    children_code.push(compile_dom_node(child, stylesheet, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
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
            for rule in &stylesheet.rules {
                let mut wrapper = MacroElementWrapper {
                    handle: handle.clone(),
                    active_classes: static_classes.clone(),
                };
                
                if rule.selector.matches(&wrapper) {
                    matched_rules.push(MatchedRule {
                        declarations: rule.declarations.iter().map(|d| (d.name.to_string(), d.value.to_string())).collect(),
                        condition: None,
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
                            });
                        }
                    }
                }
            }

            let mut rule_applications = Vec::new();
            for rule in matched_rules {
                let mut app = Vec::new();
                for (prop_name, prop_val) in rule.declarations {
                    app.push(quote! {
                        xerune::css::apply_declaration(#prop_name, #prop_val, &mut current_style, &mut layout_style);
                    });
                }
                if let Some(cond) = rule.condition {
                    let cond_tokens: proc_macro2::TokenStream = cond.parse().unwrap();
                    rule_applications.push(quote! {
                        if #cond_tokens {
                            #(#app)*
                        }
                    });
                } else {
                    rule_applications.extend(app);
                }
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
                    let val_ast = Ast::from_str(val, None, &syntax).unwrap();
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
                child_compilation.push(compile_dom_node(child, stylesheet, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
            }

            quote! {
                let (node, mut layout_style, mut current_style, parsed) = {
                    #(#dynamic_vars)*
                    let attrs_slice: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'_, str>)] = &[
                        #(#static_attrs,)*
                        #(#dynamic_attrs),*
                    ];

                    let defaults = xerune::defaults::get_default_style(#tag, &parent_style);
                    let mut layout_style = defaults.taffy_style;
                    let mut current_style = defaults.container_style;

                    // Match and apply CSS rules
                    #(#rule_applications)*

                    // Match and apply inline/dynamic attributes
                    let mut parsed = xerune::ui::attributes::ParsedAttributes::new(defaults.element_type);
                    xerune::ui::attributes::parse_attributes_generic(
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
                xerune::ui::style_resolution::finalize_node_style(
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
                    current_style.border_width = 0.0;
                    current_style.border_radius = 0.0;
                    current_style.border_color = None;
                    current_style.overflow = xerune::Overflow::Visible;
                    current_style.animation_name = None;
                    current_style.animation_duration = 0.0;
                    current_style.animation_timing_function = std::sync::Arc::from("ease");
                    current_style.animation_delay = 0.0;
                    current_style.animation_iteration_count = xerune::style::AnimationIterationCount::Count(1.0);
                    current_style.animation_direction = std::sync::Arc::from("normal");
                    current_style.animation_fill_mode = std::sync::Arc::from("none");
                    current_style.animation_play_state = std::sync::Arc::from("running");

                    let normalized = xerune::ui::normalize_text(#trimmed);
                    let (width, height) = measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                    let text_layout_style = taffy::style::Style {
                        size: taffy::geometry::Size { 
                            width: taffy::style::Dimension::length(width), 
                            height: taffy::style::Dimension::length(height) 
                        },
                        ..taffy::style::Style::default()
                    };
                    let node_id = builder.taffy.new_leaf(text_layout_style.clone()).unwrap();
                    builder.render_data.insert(node_id, xerune::style::RenderData::Text(normalized.into_owned(), current_style.clone()));
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

// Recursive include resolver to inline included templates at compile time at string level
fn resolve_includes_text(content: &str, cargo_manifest_dir: &std::path::Path, tracked_paths: &mut Vec<String>) -> String {
    let mut result = String::new();
    let mut remaining = content;
    while let Some(start_idx) = remaining.find("{% include") {
        result.push_str(&remaining[..start_idx]);
        let rest = &remaining[start_idx..];
        if let Some(end_idx) = rest.find("%}") {
            let tag = &rest[..end_idx + 2];
            let parts: Vec<&str> = tag.split_whitespace().collect();
            if parts.len() >= 3 {
                let quoted_path = parts[2];
                let path_str = quoted_path.trim_matches(|c| c == '"' || c == '\'');
                let mut full_path = cargo_manifest_dir.to_path_buf();
                full_path.push("templates");
                full_path.push(path_str);
                
                tracked_paths.push(path_str.to_string());
                
                let included_content = std::fs::read_to_string(&full_path)
                    .unwrap_or_else(|_| panic!("Failed to read included template file at {:?}", full_path));
                let resolved_included = resolve_includes_text(&included_content, cargo_manifest_dir, tracked_paths);
                result.push_str(&resolved_included);
            }
            remaining = &rest[end_idx + 2..];
        } else {
            result.push_str(rest);
            remaining = "";
        }
    }
    result.push_str(remaining);
    result
}

#[proc_macro_derive(XeruneTemplate, attributes(template))]
pub fn derive_xerune_template(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // Find the template path from attributes
    let mut template_path = None;
    for attr in &input.attrs {
        if attr.path().is_ident("template") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("path") {
                    let value = meta.value()?;
                    let path: syn::LitStr = value.parse()?;
                    template_path = Some(path.value());
                    Ok(())
                } else {
                    Err(meta.error("unsupported attribute"))
                }
            });
        }
    }

    let template_path = match template_path {
        Some(p) => p,
        None => panic!("XeruneTemplate requires a template(path = \"...\") attribute"),
    };

    // Load template file
    let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cargo_manifest_path = PathBuf::from(&cargo_manifest_dir);
    let mut path = cargo_manifest_path.clone();
    path.push("templates");
    path.push(&template_path);
    let template_content = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("Failed to read template file at {:?}", path));

    let mut tracked_paths = vec![template_path.clone()];

    // Resolve includes at string level
    let resolved_content = resolve_includes_text(&template_content, &cargo_manifest_path, &mut tracked_paths);
    let template_content_ref: &'static str = Box::leak(resolved_content.into_boxed_str());

    tracked_paths.sort();
    tracked_paths.dedup();
    let dummy_includes = tracked_paths.iter().map(|p| {
        let relative_path = format!("/templates/{}", p);
        quote! {
            const _: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), #relative_path));
        }
    });

    // Parse template using askama_parser
    let syntax = Syntax::default();
    let ast = Ast::from_str(template_content_ref, None, &syntax).expect("Failed to parse template AST");

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

    let stylesheet = simplecss::StyleSheet::parse(&css_content);

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

    let mut local_vars = HashSet::new();
    let body_compilation = compile_dom_node(&dom.document, &stylesheet, &mut local_vars, &dynamic_exprs, &dynamic_loops, &dynamic_ifs);

    let expanded = quote! {
        #(#dummy_includes)*

        impl xerune::ui::TemplateLayout for #name {
            fn stylesheet(&self) -> &'static str {
                ""
            }

            fn build_ui(
                &self,
                builder: &mut xerune::ui::UiBuilder,
                measurer: &impl xerune::TextMeasurer,
                default_style: &xerune::style::ContainerStyle,
                message_validator: &impl Fn(&str) -> bool,
            ) -> taffy::NodeId {
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

                    type MsgType = <#name as xerune::Model>::Message;
                    let exact = <MsgType as xerune::XeruneMessage>::VALID_EXACT;
                    let prefixes = <MsgType as xerune::XeruneMessage>::VALID_PREFIXES;

                    #(#check_calls)*
                };

                builder.keyframes = xerune::css::parse_keyframes(#css_content);
                let parent = builder.taffy.new_leaf(taffy::style::Style::default()).unwrap();
                let parent_style = default_style.clone();
                builder.render_data.insert(parent, xerune::style::RenderData::Container(parent_style.clone()));
                builder.base_styles.insert(parent, (taffy::style::Style::default(), parent_style.clone()));
                {
                    let parent = parent;
                    let parent_style = parent_style;
                    #body_compilation
                }
                parent
            }
        }
    };

    println!("EXPANDED FOR {}:\n{}", name, expanded);

    TokenStream::from(expanded)
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

        impl xerune::model::XeruneMessage for #name {
            const VALID_PREFIXES: &'static [&'static str] = &[#(#prefix_matches),*];
            const VALID_EXACT: &'static [&'static str] = &[#(#exact_matches),*];
        }
    };

    TokenStream::from(expanded)
}

