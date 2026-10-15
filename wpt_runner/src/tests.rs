use super::*;
use comparison::Fuzzy;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

/// A throwaway WPT root.
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = env::temp_dir().join(format!("xerune-wpt-{}-{}", std::process::id(), NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn load(&self, name: &str, content: &str) -> Result<Document, Problem> {
        Document::load(&self.write(name, content), &self.0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn filled(r: u8, g: u8, b: u8) -> Pixmap {
    let mut p = Pixmap::new(WIDTH, HEIGHT).unwrap();
    p.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    p
}

fn reference(must_match: bool, fuzzy: &str) -> Reference {
    Reference { path: "ref.html".into(), must_match, fuzzy: if fuzzy.is_empty() { Fuzzy::default() } else { Fuzzy::parse(fuzzy).unwrap() } }
}

#[test]
fn comparison_detects_a_single_channel_step() {
    let a = filled(255, 255, 255);
    let mut b = a.clone();
    b.data_mut()[0] = 254;
    let difference = compare(&a, &b);
    assert_eq!(difference, Difference { max_difference: 1, total_pixels: 1 });
    assert!(!Fuzzy::default().matches(difference));
    assert_eq!(&diff_image(&a, &b).data()[..4], &[255, 0, 0, 255]);
}

#[test]
fn fuzzy_metadata_supports_names_ranges_and_positions() {
    for value in ["10-15;2-3", "maxDifference=10-15;totalPixels=2-3", "totalPixels=2-3;10-15"] {
        let fuzzy = Fuzzy::parse(value).unwrap();
        assert!(fuzzy.matches(Difference { max_difference: 12, total_pixels: 2 }));
        assert!(!fuzzy.matches(Difference { max_difference: 9, total_pixels: 2 }));
        assert!(!fuzzy.matches(Difference { max_difference: 12, total_pixels: 4 }));
        assert!(!fuzzy.matches(Difference::default()));
    }
    assert!(Fuzzy::parse("0-15;2-3").unwrap().matches(Difference::default()));
    assert!(Fuzzy::parse("15;3").unwrap().matches(Difference { max_difference: 15, total_pixels: 3 }));
    for invalid in ["5%", "-1;2", "3-1;2", "256;1", "x=1;2", "maxDifference=1;maxDifference=2"] {
        assert!(Fuzzy::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn metadata_is_parsed_from_the_dom() {
    let fixture = Fixture::new();
    let doc = fixture.load("dir/test.html", "<!-- <link rel=match href=bogus> -->
        <LINK HREF='ref.html' data-unused=yes REL=MATCH>
        <link href=/other.html rel=mismatch>
        <meta content='maxDifference=0-10;totalPixels=0-20' name=fuzzy>
        <meta name=fuzzy content='ref.html:2;3'>").unwrap();
    assert_eq!(doc.references.len(), 2);
    assert_eq!(doc.references[0].path, Path::new("dir/ref.html"));
    assert!(doc.references[0].must_match);
    assert_eq!(doc.references[0].fuzzy, Fuzzy::parse("2;3").unwrap());
    assert_eq!(doc.references[1].path, Path::new("other.html"));
    assert!(!doc.references[1].must_match);
    assert_eq!(doc.references[1].fuzzy, Fuzzy::parse("0-10;0-20").unwrap());
}

#[test]
fn invalid_fuzzy_metadata_is_an_error() {
    let fixture = Fixture::new();
    for content in ["bad", "other.html:1;2"] {
        let doc = fixture.load("test.html", &format!("<link rel=match href=ref.html><meta name=fuzzy content='{content}'>"));
        assert!(matches!(doc, Err(Problem::Error(_))), "{content}");
    }
}

#[test]
fn one_match_and_every_mismatch_must_succeed() {
    let (red, blue) = (filled(255, 0, 0), filled(0, 0, 255));
    let pass = |refs: Vec<(Reference, Pixmap)>| references_pass(&red, &refs).0 != Verdict::Fail;
    assert!(pass(vec![(reference(true, ""), blue.clone()), (reference(true, ""), red.clone())]));
    assert!(!pass(vec![(reference(true, ""), blue.clone())]));
    assert!(pass(vec![(reference(false, ""), blue.clone())]));
    assert!(!pass(vec![(reference(true, ""), red.clone()), (reference(false, ""), red.clone())]));
    // A missing 100x100 square is 10000 pixels: only an explicit tolerance allows it.
    let mut square = filled(255, 255, 255);
    for y in 0..100 {
        for x in 0..100 {
            square.data_mut()[(y * WIDTH as usize + x) * 4..][..3].copy_from_slice(&[0, 128, 0]);
        }
    }
    let white = filled(255, 255, 255);
    assert_eq!(references_pass(&white, &[(reference(true, ""), square.clone())]).0, Verdict::Fail);
    assert_eq!(references_pass(&white, &[(reference(true, "255;10000"), square.clone())]).0, Verdict::Exact);
    assert_eq!(references_pass(&white, &[(reference(false, "255;10000"), square)]).0, Verdict::Fail);
}

/// A green 100x100 square at (10, 10), with its top row at `top_coverage`.
fn square_with_top_edge(top_coverage: f32) -> Pixmap {
    let mut p = filled(255, 255, 255);
    for y in 10..110 {
        for x in 10..110 {
            let c = if y == 10 { top_coverage } else { 1.0 };
            let blend = |v: u8| (255.0 - (255.0 - v as f32) * c).round() as u8;
            p.data_mut()[(y * WIDTH as usize + x) * 4..][..3].copy_from_slice(&[blend(0), blend(128), blend(0)]);
        }
    }
    p
}

#[test]
fn only_edge_antialiasing_is_forgiven() {
    let expected = square_with_top_edge(1.0);
    let refs = [(reference(true, ""), expected.clone())];
    assert_eq!(references_pass(&expected, &refs).0, Verdict::Exact);
    // Anti-aliased top edge: within tolerance, on an edge.
    assert_eq!(references_pass(&square_with_top_edge(0.8), &refs).0, Verdict::Antialiasing);
    // A missing edge row differs by far more.
    assert_eq!(references_pass(&square_with_top_edge(0.0), &refs).0, Verdict::Fail);
    // A slightly different color inside the square is not an edge.
    let mut tinted = expected.clone();
    for y in 40..60 {
        for x in 40..60 {
            tinted.data_mut()[(y * WIDTH as usize + x) * 4] = 20;
        }
    }
    assert_eq!(references_pass(&tinted, &refs).0, Verdict::Fail);
    // The tolerance only helps matching: any difference satisfies a mismatch.
    assert_eq!(references_pass(&square_with_top_edge(0.8), &[(reference(false, ""), expected.clone())]).0, Verdict::Exact);
    assert_eq!(references_pass(&expected, &[(reference(false, ""), expected.clone())]).0, Verdict::Fail);
}

#[test]
fn resource_paths_resolve_from_document_and_root() {
    let fixture = Fixture::new();
    let base = fixture.0.join("css/case/test.html");
    assert_eq!(document::resolve(&base, "../a%20b.png", &fixture.0).unwrap(), fixture.0.join("css/a b.png"));
    assert_eq!(document::resolve(&base, "/images/pixel.png", &fixture.0).unwrap(), fixture.0.join("images/pixel.png"));
    for unsupported in ["https://host/a.png", "//host/a.png", "image.png?x=1", "ref.html#target"] {
        assert!(matches!(document::resolve(&base, unsupported, &fixture.0), Err(Problem::Unsupported(_))));
    }
    assert!(matches!(document::resolve(&base, "../../../outside.png", &fixture.0), Err(Problem::Error(_))));
}

#[test]
fn dynamic_tests_are_skipped() {
    let fixture = Fixture::new();
    for setup in ["<script src='/resources/testharness.js'></script>", "<body onload='run()'>",
        "<html class=reftest-wait>", "<iframe src=ref.html></iframe>", "<meta name=flags content='paged'>"]
    {
        let doc = fixture.load("test.html", &format!("{setup}<link rel=match href=ref.html>")).unwrap();
        assert!(matches!(doc.check_supported(), Err(Problem::Unsupported(_))), "{setup}");
    }
    for setup in ["<style>div { animation: foo 1s; }</style>", "<div style='transition: color 1s'></div>"] {
        let doc = fixture.load("test.html", &format!("{setup}<link rel=match href=ref.html>")).unwrap();
        assert!(matches!(doc.stylesheet(&fixture.0), Err(Problem::Unsupported(_))), "{setup}");
    }
    // Ahem is what the runner renders with, so asking for it is fine.
    let doc = fixture.load("test.html", "<meta name=flags content=ahem><style>p { font: 20px/1 Ahem }</style><link rel=match href=ref.html>").unwrap();
    assert!(doc.check_supported().is_ok());
    assert!(doc.stylesheet(&fixture.0).is_ok());
}

#[test]
fn xhtml_runs_unless_it_self_closes_elements() {
    let fixture = Fixture::new();
    let doc = fixture.load("test.xht", "<html xmlns='http://www.w3.org/1999/xhtml'><head><link rel='match' href='ref.xht'/>\
        <style type='text/css'><![CDATA[ div { color: green } ]]></style></head><body><div>x</div><br/></body></html>").unwrap();
    assert!(doc.check_supported().is_ok());
    assert_eq!(doc.stylesheet(&fixture.0).unwrap().trim(), "div { color: green }");
    let doc = fixture.load("test.xht", "<html><head><link rel='match' href='ref.xht'/></head><body><div/></body></html>").unwrap();
    assert!(matches!(doc.check_supported(), Err(Problem::Unsupported(_))));
}

#[test]
fn stylesheets_keep_document_order_and_inline_imports() {
    let fixture = Fixture::new();
    fixture.write("css/support/external.css", "@import url(\"imported.css\");\ndiv { background: red }");
    fixture.write("css/support/imported.css", "div { background: yellow }");
    let doc = fixture.load("css/test.html", "<style>div { background: blue }</style>\
        <link href=support/external.css rel=stylesheet><style>div { background: green }</style>").unwrap();
    let css = doc.stylesheet(&fixture.0).unwrap();
    let order: Vec<_> = ["blue", "yellow", "red", "green"].iter().map(|c| css.find(c).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{css}");
    assert!(!css.contains("@import"));
}

#[test]
fn missing_stylesheets_are_errors() {
    let fixture = Fixture::new();
    let doc = fixture.load("test.html", "<link rel=match href=ref.html><link rel=stylesheet href=missing.css>").unwrap();
    assert!(matches!(doc.stylesheet(&fixture.0), Err(Problem::Error(_))));
}

#[test]
fn reference_files_are_not_tests() {
    for (path, is_test) in [("css/a/test.html", true), ("css/a/test.xht", true), ("css/a/test-ref.html", false),
        ("css/a/reference/x.html", false), ("css/a/support/x.html", false), ("css/a/x.css", false)]
    {
        assert_eq!(is_test_file(Path::new(path)), is_test, "{path}");
    }
}
