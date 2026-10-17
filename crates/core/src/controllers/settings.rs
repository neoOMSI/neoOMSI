use super::*;
use std::path::Path;

pub(super) const CAT: &str = "controller";

pub(super) fn dev_get(name: &str, key: &str) -> Option<::config::Value> {
    ::config::get_setting_sub(CAT, name, key)
}

pub(super) fn dev_put(name: &str, key: &str, v: Option<::config::Value>) {
    match v {
        Some(v) => ::config::set_setting_sub(CAT, name, key, v),
        None => ::config::remove_setting_sub(CAT, name, key),
    }
}

/// Force feedback and rumble on.
pub(crate) fn ff_enabled() -> bool {
    ::config::get_bool(CAT, "ff_enabled").unwrap_or(true)
}

pub(crate) fn set_ff_enabled(on: bool) {
    ::config::set_setting(CAT, "ff_enabled", on);
}

/// Force feedback the other way round, for devices without their own direction.
pub(crate) fn global_ff_invert() -> bool {
    ::config::get_bool(CAT, "ff_invert").unwrap_or(false)
}

pub(crate) fn set_global_ff_invert(on: bool) {
    ::config::set_setting(CAT, "ff_invert", on);
}

/// The configured devices (`[controller]` in the settings file), in their saved order.
pub(crate) fn read_cfg() -> Vec<DeviceCfg> {
    ::config::get_setting(CAT, "devices")
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(read_device))
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub(crate) fn read_device(name: &str) -> DeviceCfg {
    let int = |k: &str| dev_get(name, k).and_then(|v| v.as_integer());
    let flt =
        |k: &str| dev_get(name, k).and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64)));
    let mut d = DeviceCfg {
        name: name.to_string(),
        enabled: dev_get(name, "enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        second: dev_get(name, "second")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "0".into()),
        ..Default::default()
    };
    for a in 0..8 {
        let f = int(&format!("axis{a}")).unwrap_or(-1) as i32;
        let flags = int(&format!("axis{a}_flags")).unwrap_or(0) as i32;
        d.axes[a] = Func::from_code(f).map(|f| (f, flags & 1 != 0));
        d.axis_flags[a] = if cfg!(target_os = "linux") {
            flags
        } else {
            flags & !1
        };
        d.calibration[a] = dev_get(name, &format!("axis{a}_cal"))
            .and_then(|v| v.as_str().and_then(AxisCal::parse));
    }
    let n = int("buttons").unwrap_or(0).clamp(0, 512) as usize;
    for b in 0..n {
        let s = |k: String, def: &str| {
            dev_get(name, &k)
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| def.to_string())
        };
        d.buttons
            .push((s(format!("Button{b}"), ""), s(format!("Button{b}_n"), "0")));
    }
    d.ff_scale = flt("ff_scale_steer")
        .zip(flt("ff_scale_vibration"))
        .map(|(a, b)| (a as f32, b as f32));
    d.ff_invert = dev_get(name, "ff_invert").and_then(|v| v.as_bool());
    d.deadzone = flt("deadzone")
        .filter(|v| v.is_finite())
        .map(|v| (v as f32).clamp(0.0, 0.3));
    #[cfg(target_os = "linux")]
    {
        d.axis_mode = dev_get(name, "axis_mode")
            .and_then(|v| v.as_str().map(crate::evdev_axes::AxisMode::from_str))
            .unwrap_or_default();
    }
    d
}

#[cfg(target_os = "linux")]
pub(super) fn saved_axis_mode(name: &str) -> crate::evdev_axes::AxisMode {
    find_device_cfg(&read_cfg(), name).map_or_else(|| read_device(name).axis_mode, |d| d.axis_mode)
}

/// Put the devices into the settings (call `::config::save` to write them out).
pub(crate) fn write_cfg(devices: &[DeviceCfg]) {
    let names: Vec<String> = devices.iter().map(|d| d.name.clone()).collect();
    ::config::set_setting(CAT, "devices", names);
    for d in devices {
        write_device(d);
    }
}

pub(crate) fn write_device(d: &DeviceCfg) {
    use ::config::Value;
    let n = d.name.as_str();
    dev_put(n, "enabled", Some(Value::from(d.enabled)));
    dev_put(
        n,
        "second",
        Some(Value::from(if d.second.is_empty() {
            "0"
        } else {
            &d.second
        })),
    );
    for a in 0..8 {
        #[cfg(target_os = "linux")]
        let (f, flags) = (
            Func::code(d.axes[a].map(|(function, _)| function)),
            (d.axis_flags[a] & !1) | i32::from(d.axis_reversed(a)),
        );
        #[cfg(not(target_os = "linux"))]
        let (f, flags) = {
            let (f, inv) = match d.axes[a] {
                Some((f, inv)) => (Func::code(Some(f)), inv),
                None => (-1, false),
            };
            (f, d.axis_flags[a] | i32::from(inv))
        };
        dev_put(n, &format!("axis{a}"), Some(Value::from(f as i64)));
        dev_put(
            n,
            &format!("axis{a}_flags"),
            Some(Value::from(flags as i64)),
        );
        dev_put(
            n,
            &format!("axis{a}_cal"),
            d.calibration[a].map(|c| Value::from(c.line())),
        );
    }
    dev_put(n, "buttons", Some(Value::from(d.buttons.len() as i64)));
    for (b, (action, num)) in d.buttons.iter().enumerate() {
        dev_put(n, &format!("Button{b}"), Some(Value::from(action.as_str())));
        dev_put(
            n,
            &format!("Button{b}_n"),
            Some(Value::from(if num.is_empty() { "0" } else { num })),
        );
    }
    let sc = d.ff_scale.unwrap_or((1.0, 1.0));
    dev_put(n, "ff_scale_steer", Some(Value::from(sc.0 as f64)));
    dev_put(n, "ff_scale_vibration", Some(Value::from(sc.1 as f64)));
    dev_put(n, "ff_invert", d.ff_invert.map(Value::from));
    dev_put(n, "deadzone", d.deadzone.map(|v| Value::from(v as f64)));
    #[cfg(target_os = "linux")]
    dev_put(n, "axis_mode", Some(Value::from(d.axis_mode.as_str())));
}

pub(crate) fn parse_omsi_cfg(lines: &[String]) -> Vec<DeviceCfg> {
    let tag = |l: &str| l.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if tag(&lines[i]) != "[ctrl]" {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        while i < lines.len() && tag(&lines[i]) != "[ctrl]" {
            i += 1;
        }
        let block = &lines[start..i];
        let split = block
            .iter()
            .position(|l| tag(l) == "[buttons]")
            .unwrap_or(block.len());
        let (head, buttons) = block.split_at(split);
        let mut head = head.iter();
        let name = head.next().map(|l| l.trim().to_string()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let second = head.next().map(|l| l.trim().to_string()).unwrap_or_default();
        let nums: Vec<i32> = head
            .flat_map(|l| l.split_whitespace())
            .filter_map(|t| t.parse::<i32>().ok())
            .collect();
        let mut d = DeviceCfg {
            name,
            second,
            ..Default::default()
        };
        for a in 0..8 {
            let f = nums.get(a * 2).copied().unwrap_or(-1);
            let flags = nums.get(a * 2 + 1).copied().unwrap_or(0);
            d.axes[a] = Func::from_code(f).map(|f| (f, flags & 1 != 0));
            d.axis_flags[a] = if cfg!(target_os = "linux") {
                flags
            } else {
                flags & !1
            };
        }
        let mut b: Vec<&str> = buttons.iter().skip(1).map(|l| l.trim()).collect();
        while b.last().is_some_and(|l| l.is_empty()) {
            b.pop();
        }
        let count = b.first().and_then(|l| l.parse::<usize>().ok());
        if let Some(n) = count.filter(|n| b.len() > 2 * n) {
            b = b[1..1 + 2 * n].to_vec();
        } else if b.len() % 2 == 1 {
            b.remove(0);
        }
        for p in b.chunks(2) {
            let num = p.get(1).copied().filter(|s| !s.is_empty()).unwrap_or("0");
            d.buttons.push((p[0].to_string(), num.to_string()));
        }
        d.buttons.truncate(512);
        if let Some(k) = block.iter().position(|l| tag(l) == "[ffscale]") {
            let v: Vec<f32> = block[k + 1..]
                .iter()
                .take(2)
                .filter_map(|l| l.trim().parse::<f32>().ok())
                .collect();
            if let [steer, vibration] = v[..] {
                d.ff_scale = Some((steer, vibration));
            }
        }
        out.push(d);
    }
    out
}

pub(super) fn preserve_device_settings(imported: &mut DeviceCfg, existing: &DeviceCfg) {
    imported.calibration = existing.calibration;
    imported.enabled = existing.enabled;
    imported.ff_scale = imported.ff_scale.or(existing.ff_scale);
    imported.ff_invert = existing.ff_invert;
    imported.deadzone = existing.deadzone;
    #[cfg(target_os = "linux")]
    {
        imported.axis_mode = existing.axis_mode;
    }
}

/// Take the devices of OMSI's `gamectrler.cfg` into the settings: a device already set up
/// is replaced (what only neoOMSI knows - calibration, force feedback - stays), the others
/// stay as they are. Returns the names imported.
pub(crate) fn import_omsi_cfg(path: &Path) -> Result<Vec<String>, String> {
    let file = ::legacy_config::CfgFile::read(path).map_err(|e| e.to_string())?;
    let found = parse_omsi_cfg(&file.lines);
    if found.is_empty() {
        return Err(format!("{}: no controller in it", path.display()));
    }
    let mut devices = read_cfg();
    let mut names = Vec::new();
    for mut n in found {
        names.push(n.name.clone());
        let same = |d: &DeviceCfg| normalized_device_name(&d.name) == normalized_device_name(&n.name);
        if let Some(old) = devices.iter_mut().find(|d| same(d)) {
            preserve_device_settings(&mut n, old);
            *old = n;
        } else {
            devices.push(n);
        }
    }
    write_cfg(&devices);
    Ok(names)
}
