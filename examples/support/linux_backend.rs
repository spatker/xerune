use xerune::{Model, InputEvent, Runtime, TextMeasurer};
use skia_renderer::TinySkiaRenderer;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::time::Instant;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
use linuxfb::Framebuffer;
#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
use evdev::{Device, InputEventKind, Key, AbsoluteAxisType};

pub fn run_app<M: Model + 'static, TM: TextMeasurer + 'static>(
    title: &str, // Unused in FB
    width: u32,
    height: u32,
    mut runtime: Runtime<M, TM>,
    fonts: &'static [Font],
    _tick_interval: Option<std::time::Duration>,
) -> anyhow::Result<()> {
    
    // Attempt to open framebuffer
    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
        println!("Initializing Framebuffer Backend...");
        let mut fb = Framebuffer::new("/dev/fb0").map_err(|e| anyhow::anyhow!("Failed to open framebuffer: {}", e))?;
        
        let w = fb.var_screen_info.xres as u32;
        let h = fb.var_screen_info.yres as u32;
        let bpp = fb.var_screen_info.bits_per_pixel;
        
        println!("Framebuffer: {}x{} @ {}bpp", w, h, bpp);
        
        // Map memory
        let _ = fb.map().map_err(|e| anyhow::anyhow!("Failed to map framebuffer: {}", e))?;

        // Initialize Input
        // Scan for touch devices? Or just take the first one?
        // Typically /dev/input/eventX.
        // For now, let's try to find a device with Absolute Touch axes.
        let mut touch_device: Option<Device> = None;
         for id in 0..10 {
            let path = format!("/dev/input/event{}", id);
            if let Ok(dev) = Device::open(&path) {
                if dev.supported_absolute_axes().map(|axes| axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X)).unwrap_or(false) {
                    println!("Found touch device: {} ({})", dev.name().unwrap_or("?"), path);
                    touch_device = Some(dev);
                    break;
                }
            }
        }
        
        // Loop
        runtime.set_size(w as f32, h as f32);
        
        let mut pixmap = Pixmap::new(w, h).ok_or(anyhow::anyhow!("Failed to create pixmap"))?;
        
        let mut last_render_time: Option<f32> = None;
        let mouse_x = 0.0;
        let mouse_y = 0.0;

        loop {
            // Poll Input
            if let Some(ref mut dev) = touch_device {
                 match dev.fetch_events() {
                     Ok(events) => {
                         for ev in events {
                             println!("Input Event: {:?}", ev);
                             // Logic to update mouse_x/y and trigger Click/Hover/Scroll
                         }
                     },
                     Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {},
                     Err(e) => eprintln!("Input error: {}", e),
                 }
            }

            // Update
            let dirty = runtime.handle_event(InputEvent::Tick { render_time_ms: last_render_time });
            
            // Draw
            if dirty {
                pixmap.fill(Color::from_rgba8(0, 0, 0, 255));
                let mut renderer = TinySkiaRenderer::new(&mut pixmap, fonts);
                
                let start_render = Instant::now();
                runtime.render(&mut renderer);
                last_render_time = Some(start_render.elapsed().as_secs_f32() * 1000.0);
                
                // Blit to FB
                // This assumes 32bpp BGRA or RGBA. LinuxFB is usually BGRA or BGRx.
                // tiny-skia is Premultiplied RGBA.
                // Needs conversion.
                let data = pixmap.data();
                // fb.write_frame(data); // Hypothetical, need manual write or slice copy
                
                // This part is hardware dependent and mock for now as I can't test.
            }
            
            // Frame limiting?
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        println!("Linux framebuffer backend not enabled or not on Linux. Returning error.");
        Err(anyhow::anyhow!("Linux backend disabled"))
    }
}
