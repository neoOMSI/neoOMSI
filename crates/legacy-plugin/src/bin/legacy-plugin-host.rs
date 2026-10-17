//! Runs one OMSI plugin library for the game (see `::legacy_plugin::Remote`): built for 32-bit
//! Windows (`i686-pc-windows-*`, shipped as `legacy-plugin-host32.exe`) it loads the 32-bit
//! DLLs OMSI's plugins are; on Windows the game starts it directly, elsewhere through Wine.
//!
//! usage: legacy-plugin-host <library>   (then the protocol of `::legacy_plugin::wire` on stdio)
use legacy_plugin::{Library, wire};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

/// Own copies of stdin and stdout: a plugin may reopen them (GPM.dll sends stdout to its log).
fn own_pipes() -> std::io::Result<(File, File)> {
    let (stdin, stdout) = (std::io::stdin(), std::io::stdout());
    #[cfg(windows)]
    let (input, output) = {
        use std::os::windows::io::AsHandle;
        (
            stdin.as_handle().try_clone_to_owned()?,
            stdout.as_handle().try_clone_to_owned()?,
        )
    };
    #[cfg(unix)]
    let (input, output) = {
        use std::os::fd::AsFd;
        (
            stdin.as_fd().try_clone_to_owned()?,
            stdout.as_fd().try_clone_to_owned()?,
        )
    };
    Ok((File::from(input), File::from(output)))
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: legacy-plugin-host <library>");
        std::process::exit(2);
    };
    let (input, out) = match own_pipes() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("legacy-plugin-host: {e}");
            std::process::exit(2);
        }
    };
    let mut input = BufReader::new(input);
    let mut out = BufWriter::new(out);
    let lib = match Library::load(std::path::Path::new(&path)) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("legacy-plugin-host: {path}: {e}");
            None
        }
    };
    loop {
        let Ok(op) = wire::get_u8(&mut input) else {
            return;
        };
        let res = match (op, &lib) {
            (wire::START, Some(lib)) => {
                lib.start();
                let p = lib.procs();
                let flags = p.variable as u8
                    | (p.trigger as u8) << 1
                    | (p.system as u8) << 2
                    | (p.string as u8) << 3;
                wire::put_u8(&mut out, 1).and_then(|_| wire::put_u8(&mut out, flags))
            }
            (wire::START, None) => {
                let _ = wire::put_u8(&mut out, 0)
                    .and_then(|_| wire::put_u8(&mut out, 0))
                    .and_then(|_| out.flush());
                return;
            }
            (wire::FRAME, Some(lib)) => match wire::get_frame(&mut input) {
                Ok(f) => wire::put_reply(&mut out, &lib.frame(&f)),
                Err(_) => return,
            },
            (wire::FINALIZE, Some(lib)) => {
                lib.finalize();
                let _ = wire::put_u8(&mut out, 1).and_then(|_| out.flush());
                return;
            }
            _ => return,
        };
        if res.and_then(|_| out.flush()).is_err() {
            return;
        }
    }
}


/// Fallback for older i686 MinGW toolchains that cannot
/// supply _Unwind_Resume.
///
/// Enabled only when the build script detects that the
/// symbol is missing. Modern MinGW builds use the
/// toolchain's own implementation.
#[cfg(all(
    target_os = "windows",
    target_arch = "x86",
    target_env = "gnu",
    legacy_unwind_fallback
))]
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume() -> ! {
    std::process::abort()
}
