pub mod graphics;
pub mod style;
pub mod model;
pub mod ui;
pub mod runtime;

pub mod css;
pub mod defaults;

pub use graphics::{Color, LinearGradient, Rect, Canvas, Context, DrawCommand, TextMeasurer, Renderer};
pub use style::{Overflow, ContainerStyle, RenderData};
pub use model::{Model, InputEvent};
pub use ui::{Interaction, Ui};
pub use runtime::Runtime;

#[cfg(test)]
mod tests {
    use super::*;
    use taffy::prelude::TaffyMaxContent;

    struct MockModel;
    #[derive(Debug, PartialEq)]
    enum MockMsg {
        Tick,
    }
    impl std::str::FromStr for MockMsg {
        type Err = ();
        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s {
                "tick" => Ok(MockMsg::Tick),
                _ => Err(()),
            }
        }
    }

    impl Model for MockModel {
        type Message = MockMsg;
        
        fn view(&self) -> String {
            // Use simple structure to ensure deterministic NodeIds
            r#"
            <div style="height: 100px; overflow: scroll;">
                <div style="height: 200px; flex-shrink: 0;" data-on-click="test_interaction">Content</div>
            </div>
            "#.to_string()
        }
        fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
    }

    struct MockMeasurer;
    impl TextMeasurer for MockMeasurer {
        fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (10.0, 10.0)
        }
    }

    #[test]
    fn test_scroll_persistence() {
        let model = MockModel;
        let measurer = MockMeasurer;
        let mut runtime = Runtime::new(model, measurer);
        
        // Initial layout
        runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);

        // Scroll
        let handled = runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -10.0 // Scroll down 10px
        });
        
        assert!(handled, "Scroll event should be handled");
        
        // Verify offset
        let offsets = &runtime.scroll_offsets;
        let offset = offsets.values().next().expect("Should have scroll offset");
        assert_eq!(offset.1, 10.0, "Offset should be 10.0 after first scroll");
        
        // Trigger UI Recreation via Tick
        runtime.handle_event(InputEvent::Message("tick".to_string()));
        
        // Verify persistence
        let offsets_after = &runtime.scroll_offsets;
        let offset_after = offsets_after.values().next().expect("Should have scroll offset after tick");
        assert_eq!(offset_after.1, 10.0, "Offset should persist after Tick/Ui Recreation");
        
        // Scroll more
        runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -10.0 
        });
        
        let offsets_final = &runtime.scroll_offsets;
        let offset_final = offsets_final.values().next().expect("Should have scroll offset");
        assert_eq!(offset_final.1, 20.0, "Offset should accumulate (10+10=20)");

        // Test Clamping (Content height 200, Container 100 -> Max scroll 100)
        // Try scrolling to 200
         runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -500.0 // Big scroll down
        });
        
        // Should clamp to 100.0
        let offsets_clamped = &runtime.scroll_offsets;
        let offset_clamped = offsets_clamped.values().next().expect("Should have scroll offset");
        assert_eq!(offset_clamped.1, 100.0, "Offset should be clamped to max scroll (100.0)");

        // Test Hit Testing with Scroll
        // Content is at (0, 0) relative to container.
        // Container scroll is (0, 100).
        // Click at (10, 10) in window (container coords).
        // Should map to (10, 10 + 100) = (10, 110) in content.
        // Content height 200 via children.
        // So hitting child.
        
        // MockModel has data-on-click="test_interaction" on the child.
        // Hit test at (10, 10). Scroll is (0, 100).
        // Abs x=10, y=10.
        // Child abs pos = 0 - 0 = 0 (x), 0 - 100 = -100 (y).
        // Child rect = (0, -100, width?, 200).
        // y=10 is inside [-100, 100].
        // So it should hit.
        
        let hit = runtime.ui.hit_test(10.0, 10.0);
        assert!(hit.is_some(), "Should hit child content after scrolling");
        assert_eq!(hit.unwrap(), "test_interaction".to_string());
    }
}
