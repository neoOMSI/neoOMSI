//! Stamps the build with its version (`MAJOR.MINOR.COMMIT`, see docs/RELEASING.md) and the
//! commit it came from, so a screenshot or a log line says which version is running; on
//! Windows it also puts the application icon into the executable.

use std::process::Command;

fn main() {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let date =
        git(&["log", "-1", "--format=%cd", "--date=format:%Y-%m-%d %H:%M"]).unwrap_or_default();
    let dirty = git(&["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    println!(
        "cargo:rustc-env=OMSI_BUILD={hash}{} {date}",
        if dirty { "+" } else { "" }
    );
    println!("cargo:rustc-env=neoomsi_VERSION={}", version(&git));
    println!("cargo:rerun-if-env-changed=neoomsi_VERSION");
    println!("cargo:rerun-if-changed=../../VERSION");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
    windows_icon();
    // (the executable exports the two switchable-graphics hints of main.rs, see there)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        for sym in [
            "NvOptimusEnablement",
            "AmdPowerXpressRequestHighPerformance",
        ] {
            println!("cargo:rustc-link-arg-bin=neoomsi=/EXPORT:{sym},DATA");
        }
    }
}

/// A nightly version `MAJOR.MINOR.PATCH-nightly.g<hash>` from the VERSION file and the
/// commit (the CI passes the release's version in `neoomsi_VERSION`).
fn version(git: &dyn Fn(&[&str]) -> Option<String>) -> String {
    if let Ok(v) = std::env::var("neoomsi_VERSION") {
        if !v.trim().is_empty() {
            return v.trim().to_string();
        }
    }
    let base = std::fs::read_to_string("../../VERSION")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "0.0.0".into());
    match git(&["rev-parse", "--short=8", "HEAD"]).filter(|h| !h.is_empty()) {
        Some(h) => format!("{base}-nightly.g{h}"),
        None => format!("{base}-nightly"),
    }
}

fn windows_icon() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=../../assets/icons/app/neoomsi.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/icons/app/neoomsi.ico")
        .set("ProductName", "neoOMSI")
        .set("FileDescription", "neoOMSI")
        .set(
            "ProductVersion",
            &std::env::var("neoomsi_VERSION").unwrap_or_default(),
        );
    if let Err(e) = res.compile() {
        println!("cargo:warning=no icon in the executable: {e}");
    }
}
