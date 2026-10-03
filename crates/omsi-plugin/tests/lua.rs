//! Lua plugins driven as the game drives them: a fake bus, a few frames.
use omsi_plugin::{HostConfig, PluginIo, Plugins};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
struct Bus {
    vars: HashMap<String, f32>,
    strings: HashMap<String, String>,
    fired: Vec<(String, bool)>,
    messages: Vec<String>,
    vehicle: bool,
}

impl PluginIo for Bus {
    fn system(&mut self, name: &str) -> Option<f32> {
        (name == "Time").then_some(43200.0)
    }
    fn set_system(&mut self, _: &str, _: f32) {}
    fn has_vehicle(&self) -> bool {
        self.vehicle
    }
    fn var(&mut self, name: &str) -> Option<f32> {
        self.vars.get(name).copied()
    }
    fn set_var(&mut self, name: &str, v: f32) {
        self.vars.insert(name.into(), v);
    }
    fn string(&mut self, name: &str) -> Option<String> {
        self.strings.get(name).cloned()
    }
    fn set_string(&mut self, name: &str, s: &str) {
        self.strings.insert(name.into(), s.into());
    }
    fn fire(&mut self, t: &str, down: bool) {
        self.fired.push((t.into(), down));
    }
    fn dt(&self) -> f32 {
        0.5
    }
    fn vehicle_name(&self) -> Option<String> {
        Some("MAN SD202".into())
    }
    fn message(&mut self, text: &str, _: f32) {
        self.messages.push(text.into());
    }
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("omsi-lua-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn events_vars_timers_and_data() {
    let d = dir("main");
    std::fs::create_dir_all(d.join("Speedo")).unwrap();
    std::fs::write(
        d.join("Speedo/util.lua"),
        "return { double = function(x) return x * 2 end }",
    )
    .unwrap();
    std::fs::write(
        d.join("Speedo/main.lua"),
        r#"
        local util = require("util")
        omsi.data.runs = (omsi.data.runs or 0) + 1
        local ticks = 0
        omsi.on("vehicle", function(name) omsi.set_str("bus_name", name) end)
        omsi.every(1, function() ticks = ticks + 1; omsi.set_var("ticks", ticks) end)
        omsi.watch("Velocity", function(v, old) if old then omsi.message("v " .. v) end end)
        function on_frame(dt)
          omsi.set_var("doubled", util.double(omsi.var("Velocity") or 0))
          omsi.set_var("time", omsi.sys("Time"))
          if omsi.var("Velocity") == 50 then omsi.trigger("bus_horn") end
          assert(io == nil and os.execute == nil and dofile == nil)
        end
        "#,
    )
    .unwrap();
    let mut plugins = Plugins::load(&[d.clone()], &HostConfig::default());
    assert_eq!(plugins.lua.len(), 1);
    assert_eq!(plugins.lua[0].name, "Speedo");
    let mut bus = Bus {
        vehicle: true,
        ..Default::default()
    };
    for k in ["Velocity", "doubled", "ticks", "time"] {
        bus.vars.insert(k.into(), 0.0);
    }
    bus.strings.insert("bus_name".into(), String::new());
    bus.vars.insert("Velocity".into(), 20.0);
    for _ in 0..4 {
        plugins.frame(&mut bus);
    }
    assert_eq!(bus.strings["bus_name"], "MAN SD202");
    assert_eq!(bus.vars["doubled"], 40.0);
    assert_eq!(bus.vars["time"], 43200.0);
    assert_eq!(bus.vars["ticks"], 2.0);
    bus.vars.insert("Velocity".into(), 50.0);
    plugins.frame(&mut bus);
    assert_eq!(
        bus.fired,
        [
            ("bus_horn".to_string(), true),
            ("bus_horn".to_string(), false)
        ]
    );
    assert_eq!(bus.messages, ["v 50.0"]);
    plugins.finalize();
    let saved = std::fs::read_to_string(d.join("Speedo/data.save.lua")).unwrap();
    assert!(saved.contains("runs = 1"), "{saved}");
    // the next session reads it back
    let mut plugins = Plugins::load(&[d.clone()], &HostConfig::default());
    plugins.finalize();
    assert!(
        std::fs::read_to_string(d.join("Speedo/data.save.lua"))
            .unwrap()
            .contains("runs = 2")
    );
}

#[test]
fn errors_and_runaway_loops_are_contained() {
    let d = dir("bad");
    std::fs::write(
        d.join("loop.lua"),
        "function on_frame() while true do end end",
    )
    .unwrap();
    std::fs::write(
        d.join("broken.lua"),
        "function on_frame() error('boom') end",
    )
    .unwrap();
    std::fs::write(d.join("syntax.lua"), "this is not lua").unwrap();
    let mut plugins = Plugins::load(&[d.clone()], &HostConfig::default());
    assert_eq!(
        plugins.lua.len(),
        2,
        "the file that does not compile is left out"
    );
    let mut bus = Bus {
        vehicle: true,
        ..Default::default()
    };
    let t = std::time::Instant::now();
    plugins.frame(&mut bus);
    assert!(t.elapsed().as_secs_f32() < 3.0);
    for _ in 0..12 {
        plugins.frame(&mut bus);
    }
    assert!(plugins.lua.iter().all(|p| p.disabled));
    assert!(bus.messages.iter().any(|m| m.contains("boom")));
}
