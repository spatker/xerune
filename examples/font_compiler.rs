use fontdue::Font;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        println!("Usage: cargo run --example font_compiler <ttf_path> <size> <weight> [font_name]");
        return;
    }

    let ttf_path = &args[1];
    let size: f32 = args[2].parse().expect("Invalid size");
    let weight: u16 = args[3].parse().expect("Invalid weight");
    let default_name = Path::new(ttf_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("BitmapFont")
        .to_string();
    let font_name = args.get(4).unwrap_or(&default_name);

    let font_data = fs::read(ttf_path).expect("Failed to read font file");
    let font = Font::from_bytes(font_data, fontdue::FontSettings::default()).expect("Failed to parse font");

    let line_metrics = font.horizontal_line_metrics(size).expect("Failed to get line metrics");
    let line_height = line_metrics.new_line_size;

    let mut glyphs_code = String::new();

    // Compile ASCII characters from 32 to 126
    for code in 32..=126 {
        let c = code as u8 as char;
        let (metrics, bitmap) = font.rasterize(c, size);

        // Calculate layout offsets using fontdue Layout for perfect coordinate alignment
        let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        layout.reset(&fontdue::layout::LayoutSettings::default());
        layout.append(&[font.clone()], &fontdue::layout::TextStyle::new(&c.to_string(), size, 0));

        let (x_offset, y_offset) = if !layout.glyphs().is_empty() {
            let glyph = &layout.glyphs()[0];
            (glyph.x as i8, glyph.y as i8)
        } else {
            (0, 0)
        };

        let x_advance = metrics.advance_width.round() as u8;

        glyphs_code.push_str(&format!(
            "    BitmapGlyph {{\n\
                     character: {:?},\n\
                     width: {},\n\
                     height: {},\n\
                     x_offset: {},\n\
                     y_offset: {},\n\
                     x_advance: {},\n\
                     bitmap: &{:?},\n\
                 }},\n",
            c,
            metrics.width,
            metrics.height,
            x_offset,
            y_offset,
            x_advance,
            bitmap
        ));
    }

    let variable_name = format!("{}_{}", font_name.to_uppercase().replace("-", "_"), if weight > 0 { "BOLD" } else { "REGULAR" });

    println!(
        "pub static {}: BitmapFont = BitmapFont {{\n\
             name: {:?},\n\
             size: {:.1},\n\
             weight: {},\n\
             line_height: {:.1},\n\
             glyphs: &[\n\
         {}\n\
             ],\n\
         }};",
        variable_name,
        font_name,
        size,
        weight,
        line_height,
        glyphs_code
    );
}
