//! Runs WPT reftests against Xerune. Each test is rendered by Xerune and
//! compared, pixel for pixel, with Chromium's screenshots of its references.
//! See `docs/guide.md` ("WPT reftests").

mod comparison;
mod document;
mod expectations;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap, env, fs, io::Write, path::{Path, PathBuf},
    process::{Command, Stdio}, time::{Duration, Instant},
};
use xerune_render::{Fonts, SoftwareRenderer};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use rayon::prelude::*;
use tiny_skia::Pixmap;
use xerune::{Context, ImageError, ImageLoader, Model, Runtime};
use comparison::{Difference, compare, diff_image, differs_only_in_antialiasing};
use document::{Document, Problem, Reference};
use expectations::Expectations;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

struct RawHtmlModel {
    dom: RcDom,
    css: &'static str,
}

impl Model for RawHtmlModel {
    type Message = String;
    fn update(&mut self, _: String, _: &mut Context) {}
}

impl xerune::TemplateLayout for RawHtmlModel {
    fn stylesheet(&self) -> &'static str { self.css }

    fn build_ui(
        &self,
        builder: &mut xerune::ui::UiBuilder,
        _measurer: &impl xerune::TextMeasurer,
        _default_style: &xerune::style::ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        build(&self.dom.document, builder).unwrap_or_else(|| builder.create_element("html", &[]))
    }
}

fn build(handle: &Handle, builder: &mut xerune::ui::UiBuilder) -> Option<taffy::NodeId> {
    match &handle.data {
        NodeData::Document => handle.children.borrow().iter().find_map(|child| build(child, builder)),
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "style" | "script" | "head" | "link" | "meta" | "title") {
                return None;
            }
            let owned: Vec<(String, String)> = attrs.borrow().iter()
                .map(|a| (a.name.local.to_string(), a.value.to_string())).collect();
            let attrs: Vec<_> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let get = |key: &str| attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
            let number = |key: &str| get(key).and_then(|v| v.parse::<f32>().ok());
            let node = match (tag, get("type")) {
                ("input", Some("checkbox")) => builder.create_checkbox(get("checked").is_some(), &attrs),
                ("input", Some("range")) => builder.create_slider(number("value").unwrap_or(0.0), &attrs),
                ("input", Some("text") | None) => builder.create_input_text(get("value").unwrap_or(""), &attrs),
                ("progress", _) => builder.create_progress(number("value").unwrap_or(0.0), number("max").unwrap_or(1.0), &attrs),
                ("img", _) => builder.create_image(get("src").unwrap_or(""), &attrs),
                _ => builder.create_element(tag, &attrs),
            };
            builder.node_to_handle.insert(node, handle.clone());
            if let Some(meta) = builder.node_metadata.get_mut(&node) {
                meta.attrs = owned;
            }
            for child in handle.children.borrow().iter() {
                if let Some(child) = build(child, builder) {
                    builder.append_child(node, child);
                }
            }
            Some(node)
        }
        NodeData::Text { contents } => {
            let text = contents.borrow();
            if text.trim().is_empty() {
                return None;
            }
            let node = builder.create_text(&text, &[]);
            builder.node_to_handle.insert(node, handle.clone());
            Some(node)
        }
        _ => None,
    }
}

/// Loads `<img>` sources relative to the document, like a browser would.
struct WptImageLoader {
    document: PathBuf,
    root: PathBuf,
}

impl ImageLoader for WptImageLoader {
    fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
        let path = document::resolve(&self.document, src, &self.root)
            .map_err(|_| ImageError::Decode(format!("unsupported image URL: {src}")))?;
        Ok(fs::read(path)?)
    }
}

/// Ahem is the only font: every glyph is a box, so text is reproducible.
/// Chromium gets the same font (see `chromium_screenshots.mjs`).
fn load_fonts(root: &Path) -> Result<Fonts, String> {
    let path = root.join("fonts/Ahem.ttf");
    let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Bold text uses the same face.
    Fonts::ttf(&data, None).map_err(|e| e.to_string())
}

fn render(document: Document, root: &Path, fonts: &Fonts) -> Result<Pixmap, Problem> {
    let css = document.stylesheet(root)?;
    let loader = WptImageLoader { document: document.path.clone(), root: root.to_owned() };
    let model = RawHtmlModel { dom: document.dom, css: Box::leak(css.into_boxed_str()) };
    let mut runtime = Runtime::new(model, fonts.clone());
    runtime.set_image_loader(loader);
    runtime.set_size(WIDTH as f32, HEIGHT as f32);
    runtime.wait_for_images(Duration::from_secs(5));
    let mut buffer = vec![0xffff_ffffu32; (WIDTH * HEIGHT) as usize];
    let mut target = xerune::Surface::new(xerune::Pixels::Xrgb8888(&mut buffer), WIDTH, HEIGHT);
    runtime.render(&mut SoftwareRenderer::new(fonts.clone()), &mut target);
    let mut pixmap = Pixmap::new(WIDTH, HEIGHT).unwrap();
    // 0xAARRGGBB words to opaque RGBA bytes.
    for (pixel, bytes) in buffer.iter().zip(pixmap.data_mut().chunks_exact_mut(4)) {
        bytes.copy_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8, 255]);
    }
    Ok(pixmap)
}

/// Chromium screenshots, keyed by WPT checkout and Chromium version. They
/// live under `target/` and are regenerated on demand, never committed.
struct Screenshots {
    dir: PathBuf,
}

impl Screenshots {
    fn path(&self, test: &Path) -> PathBuf {
        let mut name = test.as_os_str().to_owned();
        name.push(".png");
        self.dir.join(name)
    }

    fn load(&self, test: &Path) -> Result<Pixmap, Problem> {
        let path = self.path(test);
        let pixmap = Pixmap::load_png(&path)
            .map_err(|e| Problem::Error(format!("no Chromium screenshot {}: {e}", path.display())))?;
        if (pixmap.width(), pixmap.height()) != (WIDTH, HEIGHT) {
            return Err(Problem::Error(format!("{} is not {WIDTH}x{HEIGHT}", path.display())));
        }
        Ok(pixmap)
    }

    fn ensure(&self, files: impl IntoIterator<Item = PathBuf>, root: &Path) -> Result<(), String> {
        let mut missing: Vec<_> = files.into_iter().filter(|f| !self.path(f).exists()).collect();
        missing.sort();
        missing.dedup();
        if missing.is_empty() {
            return Ok(());
        }
        eprintln!("Taking {} Chromium screenshots into {}", missing.len(), self.dir.display());
        let mut child = Command::new("node")
            .arg(Path::new(MANIFEST_DIR).join("chromium_screenshots.mjs"))
            .arg(root).arg(&self.dir)
            .stdin(Stdio::piped())
            .spawn().map_err(|e| format!("cannot run node: {e}"))?;
        let mut stdin = child.stdin.take().unwrap();
        for file in &missing {
            writeln!(stdin, "{}", file.display()).map_err(|e| e.to_string())?;
        }
        drop(stdin);
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("chromium_screenshots.mjs failed: {status}"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Pass,
    /// Passes once edge anti-aliasing differences are ignored.
    PassAntialiasing,
    Fail(String),
    Skip(String),
}

struct Test {
    /// Relative to the WPT root.
    path: PathBuf,
    references: Vec<Reference>,
}

/// How a render compares with its references.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// Every reference requirement is met exactly (or within `fuzzy`).
    Exact,
    /// Met once anti-aliasing differences on edges are ignored.
    Antialiasing,
    Fail,
}

/// WPT's rule: every mismatch reference must differ, and if there are match
/// references, at least one must match.
fn references_pass(test: &Pixmap, references: &[(Reference, Pixmap)]) -> (Verdict, Option<Difference>) {
    let mut best: Option<Verdict> = None;
    let mut closest: Option<Difference> = None;
    for (reference, image) in references {
        let difference = compare(test, image);
        let verdict = if reference.fuzzy.matches(difference) {
            Verdict::Exact
        } else if differs_only_in_antialiasing(test, image) {
            Verdict::Antialiasing
        } else {
            Verdict::Fail
        };
        if reference.must_match {
            if best.is_none_or(|b| verdict_rank(verdict) > verdict_rank(b)) {
                best = Some(verdict);
            }
            if closest.is_none_or(|c| difference.total_pixels < c.total_pixels) {
                closest = Some(difference);
            }
        } else if verdict == Verdict::Exact {
            // Mismatch tests often hinge on small differences, so only an
            // exact (or fuzzy) match counts as "the same image" here.
            return (Verdict::Fail, None);
        }
    }
    (best.unwrap_or(Verdict::Exact), closest)
}

fn verdict_rank(v: Verdict) -> u8 {
    match v { Verdict::Fail => 0, Verdict::Antialiasing => 1, Verdict::Exact => 2 }
}

fn run(test: Test, root: &Path, fonts: &Fonts, screenshots: &Screenshots, images: Option<&Path>) -> Status {
    let result = (|| -> Result<Status, Problem> {
        let references = test.references.iter()
            .map(|r| Ok((r.clone(), screenshots.load(&r.path)?)))
            .collect::<Result<Vec<_>, Problem>>()?;
        // A test Chromium fails says nothing about Xerune, usually that the
        // forced font changed it.
        if references_pass(&screenshots.load(&test.path)?, &references).0 == Verdict::Fail {
            return Ok(Status::Skip("fails in Chromium".into()));
        }
        // Parsed again here: the DOM is `Rc`-based and can't cross threads.
        let document = Document::load(&root.join(&test.path), root)?;
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(document, root, fonts)))
            .map_err(|_| Problem::Error("Xerune panicked".into()))??;
        let (verdict, closest) = references_pass(&image, &references);
        if verdict == Verdict::Exact {
            return Ok(Status::Pass);
        }
        // Anti-aliasing passes are saved too, so the tolerance can be checked.
        if let Some(dir) = images {
            let dir = dir.join(if verdict == Verdict::Fail { "failures" } else { "antialiasing" }).join(&test.path);
            let _ = fs::create_dir_all(&dir);
            let _ = image.save_png(dir.join("xerune.png"));
            for (i, (_, reference)) in references.iter().enumerate() {
                let _ = reference.save_png(dir.join(format!("chromium-{i}.png")));
                let _ = diff_image(&image, reference).save_png(dir.join(format!("diff-{i}.png")));
            }
        }
        if verdict == Verdict::Antialiasing {
            return Ok(Status::PassAntialiasing);
        }
        Ok(Status::Fail(match closest {
            Some(d) => format!("{} pixels differ, by up to {}", d.total_pixels, d.max_difference),
            None => "matches a mismatch reference".into(),
        }))
    })();
    match result {
        Ok(status) => status,
        Err(Problem::Unsupported(reason)) => Status::Skip(reason),
        Err(Problem::Error(reason)) => Status::Fail(reason),
    }
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(path.extension().and_then(|e| e.to_str()), Some("html" | "htm" | "xht" | "xhtml"))
        && !path.components().any(|c| matches!(c.as_os_str().to_str(), Some("support" | "reference" | "references" | "resources")))
        && !["-ref.", "-notref.", "-noref."].iter().any(|s| name.contains(s))
}

/// `suites.txt`: one WPT directory per line, `!path` excludes a prefix.
fn read_suites(file: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let (mut include, mut exclude) = (Vec::new(), Vec::new());
    for line in text.lines().map(|l| l.split('#').next().unwrap().trim()).filter(|l| !l.is_empty()) {
        match line.strip_prefix('!') {
            Some(prefix) => exclude.push(prefix.trim().to_owned()),
            None => include.push(line.to_owned()),
        }
    }
    Ok((include, exclude))
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if is_test_file(&path) {
            out.push(path);
        }
    }
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

struct Options {
    filter: Option<String>,
    update: bool,
    images: bool,
    jobs: usize,
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options { filter: None, update: false, images: false, jobs: 4 };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--update" => options.update = true,
            "--images" => options.images = true,
            "-j" | "--jobs" => options.jobs = args.next().and_then(|j| j.parse().ok()).ok_or("-j needs a number")?,
            "-h" | "--help" => {
                println!("WPT_DIR=/path/to/wpt cargo run --release -p wpt_runner -- [FILTER] [--update] [--images] [-j N]\n\n\
                    Runs the suites in wpt_runner/suites.txt and compares the results with\n\
                    wpt_runner/expectations.txt. FILTER limits the run to paths containing it.\n\
                    --update   rewrite expectations.txt from this run (not with FILTER)\n\
                    --images   save Xerune/Chromium/diff images of failures and anti-aliasing\n\
                    \x20          passes under target/wpt/images\n\
                    -j N       rendering threads (default 4)");
                std::process::exit(0);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option {arg}")),
            _ if options.filter.is_none() => options.filter = Some(arg),
            _ => return Err("expected at most one filter".into()),
        }
    }
    if options.update && options.filter.is_some() {
        return Err("--update rewrites every expectation; run it without a filter".into());
    }
    Ok(options)
}

fn main() {
    if let Err(error) = cli() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn cli() -> Result<(), String> {
    let options = parse_args()?;
    let root = PathBuf::from(env::var("WPT_DIR").map_err(|_| "set WPT_DIR to a WPT checkout")?)
        .canonicalize().map_err(|e| format!("WPT_DIR: {e}"))?;
    let crate_dir = Path::new(MANIFEST_DIR);
    let target = crate_dir.join("../target/wpt");
    let wpt_version = command_output("git", &["-C", root.to_str().unwrap(), "rev-parse", "--short=10", "HEAD"])
        .unwrap_or_else(|| "unknown".into());
    let chromium_version = command_output(&env::var("CHROMIUM").unwrap_or_else(|_| "chromium".into()), &["--version"])
        .ok_or("cannot run `chromium --version`; set CHROMIUM to the browser binary")?;
    let chromium_dir_name: String = chromium_version.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
    let screenshots = Screenshots { dir: target.join("chromium").join(format!("{wpt_version}-{chromium_dir_name}")) };
    let fonts = load_fonts(&root)?;

    let (suites, excluded) = read_suites(&crate_dir.join("suites.txt"))?;
    let mut files = Vec::new();
    for suite in &suites {
        collect(&root.join(suite), &mut files);
    }
    let relative = |p: &Path| p.strip_prefix(&root).unwrap().to_owned();
    files.retain(|f| {
        let r = relative(f).to_string_lossy().into_owned();
        !excluded.iter().any(|e| r.starts_with(e.as_str()))
            && options.filter.as_ref().is_none_or(|filter| r.contains(filter.as_str()))
    });
    files.sort();
    let start = Instant::now();

    // Parse everything first so missing screenshots are taken in one batch.
    let mut results: BTreeMap<PathBuf, Status> = BTreeMap::new();
    let mut tests = Vec::new();
    let mut not_reftests = 0;
    for file in files {
        let path = relative(&file);
        let document = match Document::load(&file, &root) {
            Ok(d) => d,
            Err(Problem::Unsupported(r)) => { results.insert(path, Status::Skip(r)); continue }
            Err(Problem::Error(r)) => { results.insert(path, Status::Fail(r)); continue }
        };
        if document.references.is_empty() {
            // testharness.js tests and references; not counted at all.
            not_reftests += 1;
            continue;
        }
        if let Err(Problem::Unsupported(r) | Problem::Error(r)) = document.check_supported() {
            results.insert(path, Status::Skip(r));
            continue;
        }
        tests.push(Test { path, references: document.references });
    }
    screenshots.ensure(
        tests.iter().flat_map(|t| std::iter::once(t.path.clone()).chain(t.references.iter().map(|r| r.path.clone()))),
        &root,
    )?;

    let failures_dir = options.images.then(|| target.join("images"));
    if let Some(dir) = &failures_dir {
        let _ = fs::remove_dir_all(dir);
    }
    let pool = rayon::ThreadPoolBuilder::new().num_threads(options.jobs).build().map_err(|e| e.to_string())?;
    let rendered: Vec<(PathBuf, Status)> = pool.install(|| tests.into_par_iter().map(|test| {
        let path = test.path.clone();
        (path, run(test, &root, &fonts, &screenshots, failures_dir.as_deref()))
    }).collect());
    results.extend(rendered);

    report(&results, &suites, not_reftests, start.elapsed());
    let expectations_file = crate_dir.join("expectations.txt");
    let expectations = Expectations::read(&expectations_file)?;
    let changes = expectations.compare(&results);
    changes.print();
    if options.update {
        let header = format!("WPT {wpt_version}, {chromium_version}");
        Expectations::write(&expectations_file, &header, &suites, &results)?;
        println!("Updated {}", expectations_file.display());
    } else if changes.has_regressions() {
        std::process::exit(1);
    }
    Ok(())
}

fn suite_of<'a>(path: &Path, suites: &'a [String]) -> &'a str {
    let path = path.to_string_lossy();
    suites.iter().filter(|s| path.starts_with(s.as_str())).max_by_key(|s| s.len()).map_or("?", |s| s.as_str())
}

fn report(results: &BTreeMap<PathBuf, Status>, suites: &[String], not_reftests: usize, elapsed: Duration) {
    for (path, status) in results {
        match status {
            Status::Pass | Status::PassAntialiasing => {}
            Status::Fail(reason) => println!("FAIL {}: {reason}", path.display()),
            Status::Skip(reason) => println!("SKIP {}: {reason}", path.display()),
        }
    }
    let mut skip_reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for status in results.values() {
        if let Status::Skip(reason) = status {
            *skip_reasons.entry(reason.split(':').next().unwrap()).or_default() += 1;
        }
    }
    let mut reasons: Vec<_> = skip_reasons.into_iter().collect();
    reasons.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("\nSkipped (needs more than static HTML/CSS, or fails in Chromium):");
    for (reason, n) in reasons.iter().take(12) {
        println!("  {n:6}  {reason}");
    }
    println!("\n{}", expectations::summary_table(results, suites, false));
    println!("{not_reftests} files without references (testharness tests, references) were not run. {:.1?}", elapsed);
}
