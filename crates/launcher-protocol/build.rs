fn main() {
    println!("cargo:rerun-if-changed=proto");
    let files = protox::compile(["launcher.proto", "game_link.proto"], ["proto"]).unwrap_or_else(|e| panic!("{e:?}"));
    let commands: Vec<String> = files
        .file
        .iter()
        .flat_map(|f| &f.message_type)
        .filter(|m| m.name() == "Request")
        .flat_map(|m| &m.field)
        .map(|f| f.name())
        .filter(|n| !matches!(*n, "handshake" | "shutdown"))
        .map(|n| format!("{n:?}"))
        .collect();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(
        out.join("commands.rs"),
        format!("pub const COMMANDS: &[&str] = &[{}];\n", commands.join(", ")),
    )
    .expect("OUT_DIR is writable");
    prost_build::Config::new()
        .compile_fds(files)
        .expect("the protocol's Rust types");
}
