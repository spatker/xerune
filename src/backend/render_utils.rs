/// Translate touch/mouse screen space coordinates `(touch_x, touch_y)` to logical layout coordinates based on display rotation.
pub fn map_touch_to_logical(touch_x: f32, touch_y: f32, disp_w: f32, disp_h: f32, rotation: u32) -> (f32, f32) {
    match rotation {
        90 => (touch_y, disp_w - 1.0 - touch_x),
        180 => (disp_w - 1.0 - touch_x, disp_h - 1.0 - touch_y),
        270 => (disp_h - 1.0 - touch_y, touch_x),
        _ => (touch_x, touch_y),
    }
}
