use crate::*;

pub static ADAPTER_TEXTURE_MB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn wait_gpu(
    device: &wgpu::Device,
    submission: Option<wgpu::SubmissionIndex>,
) -> Result<(), wgpu::PollError> {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: submission,
            timeout: None,
        })
        .map(|_| ())
}

pub(crate) fn dedicated_vram_mb(info: &wgpu::AdapterInfo) -> Option<u64> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
        let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            i += 1;
            let Ok(d) = a.GetDesc1() else { continue };
            if d.VendorId == info.vendor && d.DeviceId == info.device {
                return Some(d.DedicatedVideoMemory as u64 >> 20);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = info;
        None
    }
}

pub(crate) struct DevicePoller {
    pub(crate) stop: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl DevicePoller {
    pub(crate) fn start(device: &wgpu::Device) -> Option<Self> {
        if cfg!(target_arch = "wasm32") || ::legacy_config::env::var_os("OMSI_NO_POLL_THREAD").is_some() {
            return None;
        }
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (device, flag) = (device.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("omsi-gpu-poll".into())
            .spawn(move || {
                // (every 4 ms: the frame's own submits run the same upkeep, this thread only
                // takes the freeing of finished work off the frame's thread; every millisecond,
                // a few dozen times a frame, it fought the frame's thread for the queue's lock)
                let pause = std::time::Duration::from_millis(4);
                while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = device.poll(wgpu::PollType::Poll);
                    std::thread::sleep(pause);
                }
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DevicePoller {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub(crate) fn gpu_error_text(e: &wgpu::Error) -> String {
    match e {
        wgpu::Error::Validation { description, .. } | wgpu::Error::Internal { description, .. } => {
            description.trim().replace('\n', " ")
        }
        other => other.to_string(),
    }
}
