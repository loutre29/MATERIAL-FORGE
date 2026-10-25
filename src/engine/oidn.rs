//! Intel Open Image Denoise (Apache License 2.0, `third_party/oidn`),
//! loaded at runtime: the render mode's denoiser. It runs on its own thread
//! (OIDN picks the GPU through CUDA or HIP when it can, else the CPU) and
//! takes the path tracer's radiance with the first-hit albedo and normal as
//! guides. Without the DLLs the render is shown as traced (noisy).

use libloading::Library;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

type Handle = *mut c_void;

const DEVICE_DEFAULT: i32 = 0;
const DEVICE_CPU: i32 = 1;
const FORMAT_FLOAT3: i32 = 3;
const QUALITY_BALANCED: i32 = 5;
const QUALITY_HIGH: i32 = 6;

struct Api {
    _lib: Library,
    new_device: unsafe extern "C" fn(i32) -> Handle,
    commit_device: unsafe extern "C" fn(Handle),
    get_device_error: unsafe extern "C" fn(Handle, *mut *const c_char) -> i32,
    get_device_int: unsafe extern "C" fn(Handle, *const c_char) -> i32,
    release_device: unsafe extern "C" fn(Handle),
    new_buffer: unsafe extern "C" fn(Handle, usize) -> Handle,
    write_buffer: unsafe extern "C" fn(Handle, usize, usize, *const c_void),
    read_buffer: unsafe extern "C" fn(Handle, usize, usize, *mut c_void),
    release_buffer: unsafe extern "C" fn(Handle),
    new_filter: unsafe extern "C" fn(Handle, *const c_char) -> Handle,
    set_filter_image: unsafe extern "C" fn(Handle, *const c_char, Handle, i32, usize, usize, usize, usize, usize),
    set_filter_bool: unsafe extern "C" fn(Handle, *const c_char, bool),
    set_filter_int: unsafe extern "C" fn(Handle, *const c_char, i32),
    commit_filter: unsafe extern "C" fn(Handle),
    execute_filter: unsafe extern "C" fn(Handle),
    release_filter: unsafe extern "C" fn(Handle),
}

/// Folders that may hold the DLLs: `oidn` next to the executable (the
/// installer), then `third_party/oidn` above it (the repository).
fn dll_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(d) = std::env::var("MF_OIDN_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("oidn"));
        }
        v.extend(exe.ancestors().skip(1).take(4).map(|d| d.join("third_party").join("oidn")));
    }
    v
}

impl Api {
    fn load() -> Result<Self, String> {
        let dir = dll_dirs().into_iter().find(|d| d.join("OpenImageDenoise.dll").is_file()).ok_or("OpenImageDenoise.dll not found")?;
        let path = dir.join("OpenImageDenoise.dll");
        // Its own folder first, so the core, TBB and device DLLs next to it load.
        #[cfg(windows)]
        let lib: Library = unsafe {
            use libloading::os::windows::{Library as WinLib, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
            WinLib::load_with_flags(&path, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS).map_err(|e| e.to_string())?.into()
        };
        #[cfg(not(windows))]
        let lib = unsafe { Library::new(&path).map_err(|e| e.to_string())? };
        macro_rules! sym {
            ($name:literal) => {
                *unsafe { lib.get($name) }.map_err(|e| format!("{}: {e}", String::from_utf8_lossy($name)))?
            };
        }
        Ok(Api {
            new_device: sym!(b"oidnNewDevice\0"),
            commit_device: sym!(b"oidnCommitDevice\0"),
            get_device_error: sym!(b"oidnGetDeviceError\0"),
            get_device_int: sym!(b"oidnGetDeviceInt\0"),
            release_device: sym!(b"oidnReleaseDevice\0"),
            new_buffer: sym!(b"oidnNewBuffer\0"),
            write_buffer: sym!(b"oidnWriteBuffer\0"),
            read_buffer: sym!(b"oidnReadBuffer\0"),
            release_buffer: sym!(b"oidnReleaseBuffer\0"),
            new_filter: sym!(b"oidnNewFilter\0"),
            set_filter_image: sym!(b"oidnSetFilterImage\0"),
            set_filter_bool: sym!(b"oidnSetFilterBool\0"),
            set_filter_int: sym!(b"oidnSetFilterInt\0"),
            commit_filter: sym!(b"oidnCommitFilter\0"),
            execute_filter: sym!(b"oidnExecuteFilter\0"),
            release_filter: sym!(b"oidnReleaseFilter\0"),
            _lib: lib,
        })
    }

    /// The device's pending error, if any.
    fn error(&self, dev: Handle) -> Option<String> {
        let mut msg: *const c_char = std::ptr::null();
        let code = unsafe { (self.get_device_error)(dev, &mut msg) };
        if code == 0 {
            return None;
        }
        let text = if msg.is_null() { String::new() } else { unsafe { CStr::from_ptr(msg) }.to_string_lossy().into_owned() };
        Some(format!("OIDN error {code}: {text}"))
    }
}

/// One image to denoise: linear HDR radiance, albedo and normal, RGB f32.
pub struct Job {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub color: Vec<f32>,
    pub albedo: Vec<f32>,
    pub normal: Vec<f32>,
    /// Final image (high quality); otherwise balanced (faster).
    pub final_quality: bool,
}

/// The denoised radiance of job `id` (RGB f32).
pub struct Output {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub color: Vec<f32>,
}

pub struct Denoiser {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Result<Output, String>>,
    alive: Arc<AtomicBool>,
    /// Jobs sent and not answered yet.
    pending: usize,
    /// The device OIDN picked ("GPU (CUDA)", "CPU"...), once started.
    device: Arc<std::sync::Mutex<String>>,
}

impl Denoiser {
    /// Loads OIDN and starts its thread; None when the DLLs are missing.
    pub fn start() -> Result<Self, String> {
        let api = Api::load()?;
        let (tx, jobs) = mpsc::channel::<Job>();
        let (results, rx) = mpsc::channel::<Result<Output, String>>();
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let device = Arc::new(std::sync::Mutex::new(String::new()));
        let dev_name = device.clone();
        std::thread::Builder::new()
            .name("oidn".into())
            .spawn(move || {
                let r = worker(&api, jobs, &results, &dev_name);
                if let Err(e) = r {
                    let _ = results.send(Err(e));
                }
                flag.store(false, Ordering::Relaxed);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { tx, rx, alive, pending: 0, device })
    }

    /// The device in use (empty until the thread has started it).
    pub fn device(&self) -> String {
        self.device.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn pending(&self) -> bool {
        self.pending > 0
    }

    pub fn submit(&mut self, job: Job) {
        if self.tx.send(job).is_ok() {
            self.pending += 1;
        }
    }

    /// The newest finished image, if any (errors are returned once).
    pub fn poll(&mut self) -> Option<Result<Output, String>> {
        let mut last = None;
        while let Ok(r) = self.rx.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            last = Some(r);
        }
        last
    }
}

fn worker(api: &Api, jobs: mpsc::Receiver<Job>, results: &mpsc::Sender<Result<Output, String>>, device: &std::sync::Mutex<String>) -> Result<(), String> {
    // The best device (GPU through CUDA / HIP), else the CPU.
    let mut dev = unsafe { (api.new_device)(DEVICE_DEFAULT) };
    unsafe { (api.commit_device)(dev) };
    if dev.is_null() || api.error(dev).is_some() {
        if !dev.is_null() {
            unsafe { (api.release_device)(dev) };
        }
        dev = unsafe { (api.new_device)(DEVICE_CPU) };
        unsafe { (api.commit_device)(dev) };
        if let Some(e) = api.error(dev) {
            return Err(e);
        }
    }
    let name = |s: &str| CString::new(s).unwrap();
    // OIDN device types: 1 CPU, 2 SYCL, 3 CUDA, 4 HIP, 5 Metal.
    let kind = unsafe { (api.get_device_int)(dev, name("type").as_ptr()) };
    if let Ok(mut d) = device.lock() {
        *d = match kind {
            1 => "CPU",
            2 => "GPU (SYCL)",
            3 => "GPU (CUDA)",
            4 => "GPU (HIP)",
            5 => "GPU (Metal)",
            _ => "unknown device",
        }
        .to_string();
    }
    let filter = unsafe { (api.new_filter)(dev, name("RT").as_ptr()) };
    // Buffers kept while the size does not change.
    let mut bufs: Option<(usize, [Handle; 4])> = None;
    while let Ok(mut job) = jobs.recv() {
        // Only the newest job matters (older ones are for a stale image).
        let mut skipped = 0;
        while let Ok(j) = jobs.try_recv() {
            job = j;
            skipped += 1;
        }
        for _ in 0..skipped {
            let _ = results.send(Ok(Output { id: u64::MAX, width: 0, height: 0, color: Vec::new() }));
        }
        let (w, h) = (job.width as usize, job.height as usize);
        let bytes = w * h * 12;
        if job.color.len() * 4 != bytes || job.albedo.len() * 4 != bytes || job.normal.len() * 4 != bytes {
            let _ = results.send(Err("OIDN: bad image size".into()));
            continue;
        }
        if bufs.as_ref().is_none_or(|b| b.0 != bytes) {
            if let Some((_, old)) = bufs.take() {
                for b in old {
                    unsafe { (api.release_buffer)(b) };
                }
            }
            let mk = || unsafe { (api.new_buffer)(dev, bytes) };
            bufs = Some((bytes, [mk(), mk(), mk(), mk()]));
        }
        let b = bufs.as_ref().unwrap().1;
        unsafe {
            (api.write_buffer)(b[0], 0, bytes, job.color.as_ptr() as *const c_void);
            (api.write_buffer)(b[1], 0, bytes, job.albedo.as_ptr() as *const c_void);
            (api.write_buffer)(b[2], 0, bytes, job.normal.as_ptr() as *const c_void);
            for (img, buf) in [("color", b[0]), ("albedo", b[1]), ("normal", b[2]), ("output", b[3])] {
                (api.set_filter_image)(filter, name(img).as_ptr(), buf, FORMAT_FLOAT3, w, h, 0, 12, w * 12);
            }
            (api.set_filter_bool)(filter, name("hdr").as_ptr(), true);
            (api.set_filter_int)(filter, name("quality").as_ptr(), if job.final_quality { QUALITY_HIGH } else { QUALITY_BALANCED });
            (api.commit_filter)(filter);
            (api.execute_filter)(filter);
        }
        if let Some(e) = api.error(dev) {
            let _ = results.send(Err(e));
            continue;
        }
        let mut out = vec![0f32; w * h * 3];
        unsafe { (api.read_buffer)(b[3], 0, bytes, out.as_mut_ptr() as *mut c_void) };
        if results.send(Ok(Output { id: job.id, width: job.width, height: job.height, color: out })).is_err() {
            break;
        }
    }
    unsafe {
        if let Some((_, old)) = bufs.take() {
            for b in old {
                (api.release_buffer)(b);
            }
        }
        (api.release_filter)(filter);
        (api.release_device)(dev);
    }
    Ok(())
}
