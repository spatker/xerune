//! Every template of the repository parses, and its expressions survive
//! being written back as template syntax (which is how template code inside
//! an attribute value reaches the attribute's parser).

use std::path::{Path, PathBuf};

use xerune_template::{AttrPart, Cond, Expr, File, Kind, Template};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn parse_file(path: &Path) -> Template {
    let file = File {
        name: path.display().to_string(),
        dir: path.parent().unwrap().to_path_buf(),
        content: std::fs::read_to_string(path).unwrap(),
    };
    xerune_template::parse(file, &mut |include: &str, dir: &Path| {
        let path = dir.join(include);
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        Ok(File { name: include.to_string(), dir: dir.to_path_buf(), content })
    })
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn parse_source(source: &str) -> Result<Template, xerune_template::Error> {
    let file = File { name: "<test>".to_string(), dir: PathBuf::new(), content: source.to_string() };
    xerune_template::parse(file, &mut |path: &str, _: &Path| Err(format!("no include {path}")))
}

fn templates() -> Vec<PathBuf> {
    let mut paths = vec![repo().join("tests/template_grammar.html")];
    for example in std::fs::read_dir(repo().join("examples")).unwrap() {
        let dir = example.unwrap().path().join("templates");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            paths.extend(entries.map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "html")));
        }
    }
    paths.sort();
    paths
}

fn exprs_and_conds(tpl: &Template, exprs: &mut Vec<Expr>, conds: &mut Vec<Cond>) {
    fn attr_parts(parts: &[AttrPart], exprs: &mut Vec<Expr>, conds: &mut Vec<Cond>) {
        for part in parts {
            match part {
                AttrPart::Lit(_) => {}
                AttrPart::Expr(e) => exprs.push(e.clone()),
                AttrPart::If(branches) => {
                    for b in branches {
                        conds.extend(b.cond.clone());
                        attr_parts(&b.parts, exprs, conds);
                    }
                }
            }
        }
    }
    for node in &tpl.nodes {
        match &node.kind {
            Kind::Expr(e) | Kind::For(_, e) => exprs.push(e.clone()),
            Kind::Branch(cond) => conds.extend(cond.clone()),
            Kind::Element { attrs, .. } => {
                for attr in attrs {
                    attr_parts(attr.parts.as_deref().unwrap_or_default(), exprs, conds);
                }
            }
            _ => {}
        }
    }
}

#[test]
fn every_template_parses_and_round_trips() {
    let paths = templates();
    assert!(paths.len() > 5, "found only {paths:?}");
    for path in paths {
        let tpl = parse_file(&path);
        let (mut exprs, mut conds) = (Vec::new(), Vec::new());
        exprs_and_conds(&tpl, &mut exprs, &mut conds);
        for expr in exprs {
            let again = parse_source(&format!("{{{{ {expr} }}}}")).unwrap();
            let reparsed = again.nodes.iter().find_map(|n| match &n.kind {
                Kind::Expr(e) => Some(e.clone()),
                _ => None,
            });
            assert_eq!(reparsed.as_ref(), Some(&expr), "{}: `{expr}`", path.display());
        }
        for cond in conds {
            let again = parse_source(&format!("{{% if {cond} %}}x{{% endif %}}")).unwrap();
            let reparsed = again.nodes.iter().find_map(|n| match &n.kind {
                Kind::Branch(c) => c.clone(),
                _ => None,
            });
            assert_eq!(reparsed.as_ref(), Some(&cond), "{}: `{cond}`", path.display());
        }
    }
}

#[test]
fn operators_round_trip() {
    for source in ["a bitor b", "a xor b", "a bitand b", "(a,)", "-x * 2", "a..=b", "\"{:.1}\"|format(v, w)", "xs[i].f(1, 'c')"] {
        let tpl = parse_source(&format!("{{{{ {source} }}}}")).unwrap();
        let expr = tpl.nodes.iter().find_map(|n| match &n.kind {
            Kind::Expr(e) => Some(e.to_string()),
            _ => None,
        });
        assert_eq!(expr.as_deref(), Some(source));
    }
}

#[test]
fn blocks_become_nodes() {
    let tpl = parse_source(r#"<ul>{% for x in xs %}<li class="{% if x.on %}on{% endif %}">{{ x.name }}</li>{% endfor %}</ul>"#).unwrap();
    let ul = tpl.nodes.iter().position(|n| matches!(&n.kind, Kind::Element { tag, .. } if tag == "ul")).unwrap();
    let [for_node] = tpl.children(ul) else { panic!("ul has {:?}", tpl.children(ul)) };
    assert!(matches!(&tpl.node(*for_node).kind, Kind::For(..)));
    let [li] = tpl.children(*for_node) else { panic!() };
    let Kind::Element { attrs, .. } = &tpl.node(*li).kind else { panic!() };
    assert!(matches!(attrs[0].parts.as_deref(), Some([AttrPart::If(_)])));
    assert!(matches!(&tpl.node(tpl.children(*li)[0]).kind, Kind::Expr(Expr::Attr(..))));
}

#[test]
fn errors_point_at_the_template() {
    let err = parse_source("<p>\n{{ name|upper }}</p>").unwrap_err().to_string();
    assert!(err.contains("filter `|upper` is not supported"), "{err}");
    assert!(err.contains("<test>:2:4"), "{err}");
    let err = parse_source(r#"<div {% if a %}class="x"{% endif %}></div>"#).unwrap_err().to_string();
    assert!(err.contains("can only appear inside a quoted attribute value"), "{err}");
    let err = parse_source(r#"<p class="{{ c }}"></p>"#).unwrap_err().to_string();
    assert!(err.contains("can't be styled"), "{err}");
}
