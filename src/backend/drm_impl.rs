use crate::{Model, Runtime, TextMeasurer};
use super::{
    Backend, BackendError, MpscProxy,
    input::{EvdevInputSource, SurfaceInfo},
    common_loop::{FramePresenter, run_embedded_event_loop},
    render_utils::blit_rotated,
};
use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, AsRawFd};
use std::sync::mpsc::channel;
use std::thread;

use drm::control::Device as ControlDevice;
use drm::Device as BasicDevice;
use drm_fourcc::DrmFourcc;

/// File descriptor wrapper representing an opened DRI/DRM card device.
pub struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl drm::control::Device for Card {}

impl Card {
    /// Attempt to open the first available DRI card device under /dev/dri/.
    pub fn open_dri_card() -> Result<Self, BackendError> {
        for i in 0..5 {
            let path = format!("/dev/dri/card{}", i);
            if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                println!("Opened DRM device: {}", path);
                return Ok(Card(file));
            }
        }
        Err(BackendError::Init("No DRM card device found".to_string()))
    }
}

fn wait_for_page_flip(card: &Card) -> Result<(), BackendError> {
    let fd = card.as_fd().as_raw_fd();
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let ret = unsafe { libc::poll(&mut poll_fd, 1, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(BackendError::Run(format!("Poll error: {}", err)));
        }
        if ret > 0 {
            break;
        }
    }
    
    let events = card.receive_events()
        .map_err(|e| BackendError::Run(format!("Failed to receive DRM events: {:?}", e)))?;
    for event in events {
        match event {
            drm::control::Event::PageFlip(_) => {
                return Ok(());
            }
            _ => {}
        }
    }
    Ok(())
}

struct DrmPresenter<M1, M2> {
    card: Card,
    crtc_handle: drm::control::crtc::Handle,
    fb1: drm::control::framebuffer::Handle,
    fb2: drm::control::framebuffer::Handle,
    map1: M1,
    map2: M2,
    current_fb: drm::control::framebuffer::Handle,
    pending_flip: bool,
}

impl<M1, M2> FramePresenter for DrmPresenter<M1, M2>
where
    M1: std::ops::DerefMut<Target = [u8]>,
    M2: std::ops::DerefMut<Target = [u8]>,
{
    fn prepare_frame(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    fn present(&mut self, local_buffer: &[u32], surface: &SurfaceInfo) -> Result<(), BackendError> {
        if self.pending_flip {
            wait_for_page_flip(&self.card)?;
            self.pending_flip = false;
        }

        let (target_fb, draw_slice) = if self.current_fb == self.fb1 {
            (self.fb2, self.map2.as_mut())
        } else {
            (self.fb1, self.map1.as_mut())
        };

        let draw_slice_u32 = unsafe {
            std::slice::from_raw_parts_mut(
                draw_slice.as_mut_ptr() as *mut u32,
                draw_slice.len() / 4,
            )
        };

        #[cfg(feature = "profile")]
        coarse_prof::profile!("blit_rotated");
        blit_rotated(
            local_buffer,
            draw_slice_u32,
            surface.logical_w,
            surface.logical_h,
            surface.disp_w,
            surface.disp_h,
            surface.rotation,
        );

        loop {
            match self.card.page_flip(self.crtc_handle, target_fb, drm::control::PageFlipFlags::EVENT, None) {
                Ok(_) => {
                    self.current_fb = target_fb;
                    self.pending_flip = true;
                    break;
                }
                Err(e) => {
                    let err_raw = std::io::Error::from(e);
                    if err_raw.kind() == std::io::ErrorKind::WouldBlock || err_raw.raw_os_error() == Some(libc::EBUSY) {
                        thread::sleep(std::time::Duration::from_millis(1));
                    } else {
                        return Err(BackendError::Run(format!("Failed to page flip: {:?}", err_raw)));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Backend implementation for direct rendering manager (DRM/KMS) display drivers.
pub struct DrmBackend;

impl DrmBackend {
    /// Create a new DrmBackend.
    pub fn new() -> Self {
        Self
    }
}

impl Backend for DrmBackend {
    type Proxy = MpscProxy;

    fn run<M, TM, F>(
        self,
        _title: &str,
        _width: u32,
        _height: u32,
        runtime: Runtime<M, TM>,
        render_fn: F,
        setup: impl FnOnce(Self::Proxy) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) + 'static,
    {
        println!("Initializing DRM/KMS Backend...");
        let card = Card::open_dri_card()?;
        
        // Acquire DRM Master capability
        let _ = card.acquire_master_lock();
        
        let resources = card.resource_handles()
            .map_err(|e| BackendError::Init(format!("Failed to get DRM resources: {:?}", e)))?;
            
        let mut active_connector = None;
        for conn_handle in resources.connectors() {
            if let Ok(conn) = card.get_connector(*conn_handle, false) {
                if conn.state() == drm::control::connector::State::Connected {
                    active_connector = Some(conn);
                    break;
                }
            }
        }
        
        let connector = active_connector.ok_or_else(|| BackendError::Init("No connected connector found".to_string()))?;
        let mode = connector.modes().get(0).copied()
            .ok_or_else(|| BackendError::Init("No modes found on connector".to_string()))?;
            
        let (disp_w, disp_h) = mode.size();
        
        let rotation = std::env::var("XERUNE_ROTATION")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let rotate = rotation == 90 || rotation == 270;
        let (w, h) = if rotate { (disp_h as u32, disp_w as u32) } else { (disp_w as u32, disp_h as u32) };
        println!("DRM Display: {}x{} (mode: {}, rotation: {}), logical size: {}x{}", disp_w, disp_h, mode.name().to_string_lossy(), rotation, w, h);
        
        let encoder_handle = connector.current_encoder().unwrap_or_else(|| {
            connector.encoders().get(0).copied().expect("No encoders found")
        });
        let encoder = card.get_encoder(encoder_handle)
            .map_err(|e| BackendError::Init(format!("Failed to get encoder: {:?}", e)))?;
        
        let crtc_handle = encoder.crtc().unwrap_or_else(|| {
            resources.crtcs().get(0).copied().expect("No CRTCs found")
        });
        
        let fmt = DrmFourcc::Argb8888;
        let mut db1 = card.create_dumb_buffer((disp_w as u32, disp_h as u32), fmt, 32)
            .map_err(|e| BackendError::Init(format!("Failed to create dumb buffer 1: {:?}", e)))?;
        let mut db2 = card.create_dumb_buffer((disp_w as u32, disp_h as u32), fmt, 32)
            .map_err(|e| BackendError::Init(format!("Failed to create dumb buffer 2: {:?}", e)))?;
            
        let fb1 = card.add_framebuffer(&db1, 32, 32)
            .map_err(|e| BackendError::Init(format!("Failed to add framebuffer 1: {:?}", e)))?;
        let fb2 = card.add_framebuffer(&db2, 32, 32)
            .map_err(|e| BackendError::Init(format!("Failed to add framebuffer 2: {:?}", e)))?;
            
        let map1 = card.map_dumb_buffer(&mut db1)
            .map_err(|e| BackendError::Init(format!("Failed to map dumb buffer 1: {:?}", e)))?;
        let map2 = card.map_dumb_buffer(&mut db2)
            .map_err(|e| BackendError::Init(format!("Failed to map dumb buffer 2: {:?}", e)))?;
            
        // Initial modeset
        card.set_crtc(crtc_handle, Some(fb1), (0, 0), &[connector.handle()], Some(mode))
            .map_err(|e| BackendError::Init(format!("Failed to perform initial modeset: {:?}", e)))?;
            
        let (msg_tx, msg_rx) = channel::<String>();
        setup(MpscProxy { sender: msg_tx });
        
        let input_source = EvdevInputSource::new();
        let presenter = DrmPresenter {
            card,
            crtc_handle,
            fb1,
            fb2,
            map1,
            map2,
            current_fb: fb1,
            pending_flip: false,
        };
        let surface = SurfaceInfo {
            logical_w: w,
            logical_h: h,
            disp_w: disp_w as u32,
            disp_h: disp_h as u32,
            rotation,
        };

        run_embedded_event_loop(runtime, render_fn, input_source, presenter, surface, msg_rx)
    }
}
