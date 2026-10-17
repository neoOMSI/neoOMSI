//! `neoomsi-launcher`: the launcher's commands for a terminal (`--cli <command> [json]`), and
//! otherwise the launcher: the one a release ships beside the game, else the game's
//! built-in window (`neoomsi --launcher`).

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "--cli").unwrap_or(false) {
        let cmd = args.get(2).cloned().unwrap_or_default();
        let arg = args.get(3).cloned().unwrap_or_else(|| "{}".into());
        match omsi_launcher_lib::cli(&cmd, &arg) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
            Err(e) => {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    let game = omsi_launcher_lib::load_config().game;
    let game = if game.trim().is_empty() {
        std::env::current_exe()
            .ok()
            .and_then(|e| {
                e.parent().map(|d| {
                    d.join(if cfg!(windows) {
                        "neoomsi.exe"
                    } else {
                        "neoomsi"
                    })
                })
            })
            .unwrap_or_default()
    } else {
        std::path::PathBuf::from(game)
    };
    match omsi_launcher_lib::start_external_launcher(&game) {
        Ok(true) => return,
        Ok(false) => {
            let expected = omsi_launcher_lib::shipped_launcher(&game)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "dist/<platform>/launcher".to_string());
            eprintln!("error: neoOMSI desktop launcher was not found at {expected}.");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("error starting launcher: {e:#}");
            std::process::exit(1);
        }
    }
}
