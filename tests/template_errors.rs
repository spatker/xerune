//! Template mistakes must fail the build with a readable, located message
//! rather than a macro panic or silently wrong output. Expected compiler
//! output lives next to each case in `tests/template_errors/*.stderr`;
//! regenerate with `TRYBUILD=overwrite cargo test --test template_errors`.

#[test]
fn template_errors() {
    trybuild::TestCases::new().compile_fail("tests/template_errors/*.rs");
}
