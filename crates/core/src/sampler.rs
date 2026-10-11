//! A sampling profiler of the main thread for Windows (dev builds only), for finding where a frame's time goes
//! without an elevated profiler: `OMSI_SAMPLE=<start s>,<length s>` (default `40,40`).
//! A thread stops the main thread every millisecond, reads its call stack with the
//! unwind tables and lets it go on; after the length it writes the functions that took the
//! most samples, by themselves and with what they call, to `sample-report.txt` in the
//! working folder (names from the program's PDB through dbghelp)

#[cfg(windows)]
pub fn start() {
    let Some(spec) = ::legacy_config::env::var("OMSI_SAMPLE").ok() else {
        return;
    };
    let mut it = spec.split(',').map(|v| v.trim().parse::<f64>().ok());
    let begin = it.next().flatten().unwrap_or(40.0);
    let length = it.next().flatten().unwrap_or(40.0);
    // SAFETY: plain call
    let tid = unsafe { win::GetCurrentThreadId() };
    let _ = std::thread::Builder::new()
        .name("omsi-sampler".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs_f64(begin));
            match sample(tid, length) {
                Ok(path) => log::info!("sampler: report written to {}", path.display()),
                Err(e) => log::warn!("sampler: {e}"),
            }
        });
}

#[cfg(not(windows))]
pub fn start() {}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetCurrentThreadId() -> u32;
        pub fn GetCurrentProcess() -> isize;
        pub fn OpenThread(access: u32, inherit: i32, id: u32) -> isize;
        pub fn SuspendThread(thread: isize) -> u32;
        pub fn ResumeThread(thread: isize) -> u32;
        pub fn GetThreadContext(thread: isize, context: *mut c_void) -> i32;
        pub fn RtlLookupFunctionEntry(pc: u64, base: *mut u64, history: *mut c_void)
        -> *mut c_void;
        pub fn RtlVirtualUnwind(
            handler_type: u32,
            base: u64,
            pc: u64,
            entry: *mut c_void,
            context: *mut c_void,
            handler_data: *mut *mut c_void,
            establisher: *mut u64,
            pointers: *mut c_void,
        ) -> *mut c_void;
        pub fn LoadLibraryA(name: *const u8) -> isize;
        pub fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
    }
    #[link(name = "winmm")]
    unsafe extern "system" {
        pub fn timeBeginPeriod(ms: u32) -> u32;
    }

    /// x64 `CONTEXT`: 1232 bytes, 16-aligned; only the flags, Rsp and Rip are read by name.
    #[repr(C, align(16))]
    pub struct Context(pub [u8; 1232]);
    pub const FLAGS: usize = 0x30;
    pub const RSP: usize = 0x98;
    pub const RIP: usize = 0xF8;
    impl Context {
        pub fn get(&self, at: usize) -> u64 {
            u64::from_le_bytes(self.0[at..at + 8].try_into().unwrap())
        }
        pub fn set(&mut self, at: usize, v: u64) {
            self.0[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
    }
}

#[cfg(windows)]
fn sample(tid: u32, length: f64) -> anyhow::Result<std::path::PathBuf> {
    use std::collections::HashMap;
    use win::*;
    const DEPTH: usize = 64;
    // THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION
    // SAFETY: plain calls
    let thread = unsafe { OpenThread(0x0002 | 0x0008 | 0x0040, 0, tid) };
    if thread == 0 {
        anyhow::bail!("could not open the main thread");
    }
    unsafe { timeBeginPeriod(1) };
    let mut stacks: HashMap<Vec<u64>, u32> = HashMap::new();
    let mut buf = [0u64; DEPTH];
    let mut ctx = Box::new(Context([0; 1232]));
    let end = std::time::Instant::now() + std::time::Duration::from_secs_f64(length);
    let mut taken = 0u32;
    while std::time::Instant::now() < end {
        let mut n = 0;
        // SAFETY: the thread handle is valid; between Suspend and Resume nothing allocates,
        // the unwinder reads the stopped thread's stack within this process
        unsafe {
            if SuspendThread(thread) == u32::MAX {
                break;
            }
            ctx.0.fill(0);
            // CONTEXT_FULL (AMD64)
            ctx.0[FLAGS..FLAGS + 4].copy_from_slice(&0x0010_000Bu32.to_le_bytes());
            if GetThreadContext(thread, ctx.0.as_mut_ptr().cast()) != 0 {
                while n < DEPTH {
                    let pc = ctx.get(RIP);
                    if pc == 0 {
                        break;
                    }
                    buf[n] = pc;
                    n += 1;
                    let mut base = 0u64;
                    let entry = RtlLookupFunctionEntry(pc, &mut base, std::ptr::null_mut());
                    if entry.is_null() {
                        // a leaf: the return address is on top of the stack
                        let sp = ctx.get(RSP);
                        if sp == 0 {
                            break;
                        }
                        ctx.set(RIP, *(sp as *const u64));
                        ctx.set(RSP, sp + 8);
                    } else {
                        let mut data = std::ptr::null_mut();
                        let mut frame = 0u64;
                        RtlVirtualUnwind(
                            0,
                            base,
                            pc,
                            entry,
                            ctx.0.as_mut_ptr().cast(),
                            &mut data,
                            &mut frame,
                            std::ptr::null_mut(),
                        );
                    }
                }
            }
            ResumeThread(thread);
        }
        if n > 0 {
            *stacks.entry(buf[..n].to_vec()).or_default() += 1;
            taken += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    // names
    type SymInit = unsafe extern "system" fn(isize, *const u16, i32) -> i32;
    type SymOpts = unsafe extern "system" fn(u32) -> u32;
    type SymFrom = unsafe extern "system" fn(isize, u64, *mut u64, *mut SymbolInfo) -> i32;
    #[repr(C)]
    struct SymbolInfo {
        size_of_struct: u32,
        type_index: u32,
        reserved: [u64; 2],
        index: u32,
        size: u32,
        mod_base: u64,
        flags: u32,
        value: u64,
        address: u64,
        register: u32,
        scope: u32,
        tag: u32,
        name_len: u32,
        max_name_len: u32,
        name: [u16; 512],
    }
    // SAFETY: dbghelp's documented functions with their documented signatures
    let names = unsafe {
        let lib = LoadLibraryA(c"dbghelp.dll".as_ptr().cast());
        if lib == 0 {
            anyhow::bail!("dbghelp.dll not found");
        }
        let init: SymInit =
            std::mem::transmute(GetProcAddress(lib, c"SymInitializeW".as_ptr().cast()));
        let opts: SymOpts =
            std::mem::transmute(GetProcAddress(lib, c"SymSetOptions".as_ptr().cast()));
        let from: SymFrom =
            std::mem::transmute(GetProcAddress(lib, c"SymFromAddrW".as_ptr().cast()));
        let process = GetCurrentProcess();
        // SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS
        opts(0x2 | 0x4);
        init(process, std::ptr::null(), 1);
        let mut names: HashMap<u64, String> = HashMap::new();
        for stack in stacks.keys() {
            for &pc in stack {
                names.entry(pc).or_insert_with(|| {
                    let mut info: Box<SymbolInfo> = Box::new(std::mem::zeroed());
                    info.size_of_struct = 88;
                    info.max_name_len = 511;
                    let mut disp = 0u64;
                    // (a return address points after the call: look the call itself up)
                    if from(process, pc - 1, &mut disp, &mut *info) != 0 {
                        let len = (info.name_len as usize).min(511);
                        String::from_utf16_lossy(&info.name[..len])
                    } else {
                        format!("?{pc:x}")
                    }
                });
            }
        }
        names
    };
    let mut own: HashMap<&str, u32> = HashMap::new();
    let mut all: HashMap<&str, u32> = HashMap::new();
    let mut callers: HashMap<(&str, &str), u32> = HashMap::new();
    for (stack, &count) in &stacks {
        *own.entry(&names[&stack[0]]).or_default() += count;
        let mut seen: Vec<&str> = Vec::with_capacity(stack.len());
        for (k, pc) in stack.iter().enumerate() {
            let name = names[pc].as_str();
            if !seen.contains(&name) {
                seen.push(name);
                *all.entry(name).or_default() += count;
            }
            if k > 0 {
                let callee = names[&stack[k - 1]].as_str();
                if callee != name {
                    *callers.entry((name, callee)).or_default() += count;
                }
            }
        }
    }
    let pct = |c: u32| c as f64 * 100.0 / taken.max(1) as f64;
    let mut out = format!("{taken} samples of the main thread over {length} s\n\n== by itself\n");
    let mut v: Vec<_> = own.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, c) in v.iter().take(150) {
        out += &format!("{:6.2}%  {name}\n", pct(*c));
    }
    out += "\n== with what it calls\n";
    let mut v: Vec<_> = all.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, c) in v.iter().take(400) {
        out += &format!("{:6.2}%  {name}\n", pct(*c));
    }
    out += "\n== caller -> callee\n";
    let mut v: Vec<_> = callers.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for ((a, b), c) in v.iter().take(800) {
        out += &format!("{:6.2}%  {a}  ->  {b}\n", pct(*c));
    }
    let path = std::path::PathBuf::from("sample-report.txt");
    std::fs::write(&path, out)?;
    Ok(path)
}
