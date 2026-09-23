use xerune::{Color, ContainerStyle, TemplateLayout, TextMeasurer, Ui, UiBuilder};

struct Measurer;

impl TextMeasurer for Measurer {
    fn measure_text(&self, _: &str, _: f32, _: u16) -> (f32, f32) {
        (0.0, 0.0)
    }
}

struct Template {
    css: &'static str,
    classes: [&'static str; 2],
}

impl TemplateLayout for Template {
    fn stylesheet(&self) -> &'static str {
        self.css
    }

    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _: &impl TextMeasurer,
        _: &ContainerStyle,
        _: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        let root = builder.create_element("main", &[]);
        for class in self.classes {
            let parent = builder.create_element("section", &[("class", class)]);
            for _ in 0..2 {
                let item = builder.create_element("div", &[("class", "item")]);
                builder.append_child(parent, item);
            }
            builder.append_child(root, parent);
        }
        root
    }
}

#[test]
fn style_cache_respects_structural_selectors_across_rebuilds() {
    let red = Some(Color::new(255, 0, 0, 255));
    let blue = Some(Color::new(0, 0, 255, 255));
    let cases = [
        (
            ".red .item { background-color: red; } .blue .item { background-color: blue; }",
            [red, red, blue, blue],
        ),
        (
            ".red > .item { background-color: red; } .blue > .item { background-color: blue; }",
            [red, red, blue, blue],
        ),
        (
            ".item + .item { background-color: red; }",
            [None, red, None, red],
        ),
        (
            ".item:first-child { background-color: red; }",
            [red, None, red, None],
        ),
    ];

    for (css, expected) in cases {
        // Reuse the stylesheet cache while changing the ancestor classes.
        for reversed in [false, true, false] {
            let template = Template {
                css,
                classes: if reversed { ["blue", "red"] } else { ["red", "blue"] },
            };
            let ui = Ui::new_compiled(&template, &Measurer, ContainerStyle::default(), &|_| true)
                .unwrap();
            let mut actual = Vec::new();
            for parent in ui.taffy.children(ui.root).unwrap() {
                for item in ui.taffy.children(parent).unwrap() {
                    actual.push(ui.render_data.get(&item).unwrap().style().background_color);
                }
            }
            let mut expected = expected;
            if reversed {
                expected.rotate_left(2);
            }
            assert_eq!(actual, expected, "stylesheet: {css}, reversed: {reversed}");
        }
    }
}
