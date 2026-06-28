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
fn compile_dom_node<'a>(
    handle: &Handle,
    local_vars: &mut HashSet<String>,
    dynamic_exprs: &HashMap<usize, &'a Expr<'a>>,
    dynamic_loops: &HashMap<usize, &'a Node<'a>>,
    dynamic_ifs: &HashMap<usize, &'a Node<'a>>,
) -> proc_macro2::TokenStream {
    match &handle.data {
        NodeData::Document => {
            let mut children_code = Vec::new();
            for child in get_node_children(handle).iter() {
                children_code.push(compile_dom_node(child, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
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
                            builder.create_text_cow(#expr_tokens.to_display_string().into_owned().into(), &[])
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
                            children_code.push(compile_dom_node(child, &mut loop_local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
                        }
                        
                        let var_tokens: proc_macro2::TokenStream = var_str.parse().unwrap();
                        let iter_tokens: proc_macro2::TokenStream = iter_str.parse().unwrap();

                        return quote! {
                            {
                                let parent = parent;
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
                            // Find matching branch tag in handle's children
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
                                    children_code.push(compile_dom_node(child, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
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

            // Create attribute constructor
            let mut static_attrs = Vec::new();
            let mut dynamic_attrs = Vec::new();
            let mut dynamic_vars = Vec::new();
            for (idx, attr) in attrs.borrow().iter().enumerate() {
                let key = attr.name.local.as_ref();
                let val = attr.value.as_ref();
                if val.contains("{%") || val.contains("{{") {
                    // Parse with askama to compile dynamic string
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
                child_compilation.push(compile_dom_node(child, local_vars, dynamic_exprs, dynamic_loops, dynamic_ifs));
            }

            // Determine if widget tag (input, checkbox, progress, slider, img, canvas)
            let is_input = tag == "input";
            let (type_attr, value_attr, checked_attr, max_attr, src_attr, id_attr) = {
                let attrs_ref = attrs.borrow();
                let type_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "type").map(|a| a.value.to_string());
                let value_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "value").map(|a| a.value.to_string());
                let checked_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "checked").map(|a| a.value.to_string());
                let max_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "max").map(|a| a.value.to_string());
                let src_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "src").map(|a| a.value.to_string());
                let id_attr = attrs_ref.iter().find(|a| a.name.local.as_ref() == "id").map(|a| a.value.to_string());
                (type_attr, value_attr, checked_attr, max_attr, src_attr, id_attr)
            };
            
            let builder_call = if is_input && type_attr.as_deref() == Some("checkbox") {
                let checked_tokens = if let Some(ref checked_val) = checked_attr {
                    if checked_val.contains("{{") {
                        let val_ast = Ast::from_str(checked_val, None, &Syntax::default()).unwrap();
                        if let Some(Node::Expr(_, expr)) = val_ast.nodes().first() {
                            let expr_str = format_expr(expr, local_vars);
                            let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                            quote! { #expr_tokens }
                        } else {
                            quote! { false }
                        }
                    } else {
                        let is_checked = checked_val != "false";
                        quote! { #is_checked }
                    }
                } else {
                    quote! { false }
                };
                quote! { builder.create_checkbox_cow(#checked_tokens, &mut attrs_slice) }
            } else if is_input && type_attr.as_deref() == Some("range") {
                let val = value_attr.as_deref().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                quote! { builder.create_slider_cow(#val, &mut attrs_slice) }
            } else if is_input && (type_attr.as_deref() == Some("text") || type_attr.is_none()) {
                let val_tokens = if let Some(ref val) = value_attr {
                    if val.contains("{%") || val.contains("{{") {
                        let val_ast = Ast::from_str(val, None, &Syntax::default()).unwrap();
                        let attr_code = generate_attr_string_code(val_ast.nodes(), local_vars);
                        quote! { std::borrow::Cow::Owned(#attr_code) }
                    } else {
                        quote! { std::borrow::Cow::Borrowed(#val) }
                    }
                } else {
                    quote! { std::borrow::Cow::Borrowed("") }
                };
                quote! { builder.create_input_text_cow(#val_tokens, &mut attrs_slice) }
            } else if tag == "progress" {
                let val_tokens = if let Some(ref val) = value_attr {
                    if val.contains("{{") {
                        let val_ast = Ast::from_str(val, None, &Syntax::default()).unwrap();
                        if let Some(Node::Expr(_, expr)) = val_ast.nodes().first() {
                            let expr_str = format_expr(expr, local_vars);
                            let expr_tokens: proc_macro2::TokenStream = expr_str.parse().unwrap();
                            quote! { #expr_tokens }
                        } else {
                            quote! { 0.0 }
                        }
                    } else {
                        let parsed_val = val.parse::<f32>().unwrap_or(0.0);
                        quote! { #parsed_val }
                    }
                } else {
                    quote! { 0.0 }
                };
                let max_val = max_attr.as_deref().and_then(|m| m.parse::<f32>().ok()).unwrap_or(1.0);
                quote! { builder.create_progress_cow(#val_tokens, #max_val, &mut attrs_slice) }
            } else if tag == "img" {
                let src_tokens = if let Some(ref val) = src_attr {
                    if val.contains("{%") || val.contains("{{") {
                        let val_ast = Ast::from_str(val, None, &Syntax::default()).unwrap();
                        let attr_code = generate_attr_string_code(val_ast.nodes(), local_vars);
                        quote! { std::borrow::Cow::Owned(#attr_code) }
                    } else {
                        quote! { std::borrow::Cow::Borrowed(#val) }
                    }
                } else {
                    quote! { std::borrow::Cow::Borrowed("") }
                };
                quote! { builder.create_image_cow(#src_tokens, &mut attrs_slice) }
            } else if tag == "canvas" {
                let id_val = id_attr.unwrap_or_default();
                quote! { builder.create_canvas_cow(std::borrow::Cow::Borrowed(#id_val), &mut attrs_slice) }
            } else {
                quote! { builder.create_element_cow(std::borrow::Cow::Borrowed(#tag), &mut attrs_slice) }
            };

            quote! {
                let node = {
                    #(#dynamic_vars)*
                    let mut attrs_slice = [
                        #(#static_attrs,)*
                        #(#dynamic_attrs),*
                    ];
                    #builder_call
                };
                builder.append_child(parent, node);
                {
                    let parent = node;
                    #(#child_compilation)*
                }
            }
        }
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return quote! {};
            }
            quote! {
                let node = builder.create_text_cow(std::borrow::Cow::Borrowed(#trimmed), &[]);
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
fn resolve_includes_text(content: &str, cargo_manifest_dir: &std::path::Path) -> String {
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
                let included_content = std::fs::read_to_string(&full_path)
                    .unwrap_or_else(|_| panic!("Failed to read included template file at {:?}", full_path));
                let resolved_included = resolve_includes_text(&included_content, cargo_manifest_dir);
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

    // Resolve includes at string level
    let resolved_content = resolve_includes_text(&template_content, &cargo_manifest_path);
    let template_content_ref: &'static str = Box::leak(resolved_content.into_boxed_str());

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

    let mut local_vars = HashSet::new();
    let body_compilation = compile_dom_node(&dom.document, &mut local_vars, &dynamic_exprs, &dynamic_loops, &dynamic_ifs);

    let expanded = quote! {
        impl xerune::ui::TemplateLayout for #name {
            fn stylesheet(&self) -> &'static str {
                #css_content
            }

            fn build_ui(&self, builder: &mut xerune::ui::UiBuilder) -> taffy::NodeId {
                let parent = builder.create_element_cow(std::borrow::Cow::Borrowed("body"), &mut []);
                #body_compilation
                parent
            }
        }
    };

    println!("EXPANDED FOR {}:\n{}", name, expanded);

    TokenStream::from(expanded)
}
