use tiny_skia::*;
use rand::prelude::*;
use std::time::{Instant, Duration};

fn main() {
    let width = 800;
    let height = 600;
    let mut pixmap = Pixmap::new(width, height).unwrap();

    let mut rng = StdRng::seed_from_u64(42);
    
    // Generate triangles and colors
    let mut tris = Vec::new();
    let mut colors = Vec::new();
    for _ in 0..100 {
        let mut pb = PathBuilder::new();
        pb.move_to(rng.gen_range(0.0..200.0), rng.gen_range(0.0..200.0));
        pb.line_to(rng.gen_range(0.0..200.0), rng.gen_range(0.0..200.0));
        pb.line_to(rng.gen_range(0.0..200.0), rng.gen_range(0.0..200.0));
        pb.close();
        let path = pb.finish().unwrap();
        
        tris.push(path);
        
        let r = rng.gen_range(0..=255);
        let g = rng.gen_range(0..=255);
        let b = rng.gen_range(0..=255);
        let a = rng.gen_range(0..=255);
        colors.push(Color::from_rgba8(r, g, b, a));
    }

    const ITERATIONS: u32 = 100;
    let mut total_duration = Duration::ZERO;
    

    for _ in 0..ITERATIONS {
        // Clear pixmap (equivalent to ctx.clear_all())
        pixmap.fill(Color::TRANSPARENT);
        
        let start = Instant::now();

        for i in 0..tris.len() {
            let mut paint = Paint::default();
            paint.set_color(colors[i]);
            paint.anti_alias = false;

            pixmap.fill_path(
                &tris[i],
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        total_duration += start.elapsed();
    }

    println!("Average time for 100 triangles over {} iterations: {} microseconds.", 
             ITERATIONS, total_duration.as_micros());
}
