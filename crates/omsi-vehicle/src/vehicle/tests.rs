use super::*;

#[test]
fn axles_with_and_without_keywords() {
    let keyed = "[newachse]\nachse_long\n2.943\nachse_raddurchmesser\n1.023\n1.05\nachse_feder\n240\nachse_antrieb\n0\n[newachse]\n-2.577\n2.4\n1.4\n1.023\n280\n116\n20\n1\n0.015\n\n[mass]\n10.9\n[cog]\n0\n0.2\n0.8\n";
    let v = Vehicle::parse(&CfgFile::from_str("x.bus", keyed));
    assert_eq!(v.axles.len(), 2);
    assert_eq!(
        (
            v.axles[0].long,
            v.axles[0].wheel_diameter,
            v.axles[0].spring,
            v.axles[0].driven
        ),
        (2.943, 1.023, 240.0, false)
    );
    let b = &v.axles[1];
    assert_eq!(
        (
            b.long,
            b.max_width,
            b.min_width,
            b.wheel_diameter,
            b.spring,
            b.max_force,
            b.damper,
            b.driven,
            b.inertia_inv
        ),
        (-2.577, 2.4, 1.4, 1.023, 280.0, 116.0, 20.0, true, 0.015)
    );
    assert_eq!(v.mass, 10.9);
    assert_eq!(v.cog, Some([0.0, 0.2, 0.8]));
}

#[test]
fn a_share_of_the_drive_is_a_driven_axle() {
    let text = "[newachse]\nachse_long\n-2.9\nachse_antrieb\n0.2\n[newachse]\nachse_long\n2.9\nachse_antrieb\n0\n";
    let v = Vehicle::parse(&CfgFile::from_str("x.bus", text));
    assert_eq!(
        v.axles.iter().map(|a| a.driven).collect::<Vec<_>>(),
        vec![true, false]
    );
}

#[test]
fn rear_sections_are_not_listed_and_lead_to_their_front() {
    let dir = std::env::temp_dir().join(format!("omsi_vehicle_couple_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("G Main.bus"), "[friendlyname]\nMB\nO530G\nDefault\n\n[coupling_back]\n0\n-4\n0.3\n\n[couple_back]\ng trail.BUS\nfalse\n").unwrap();
    std::fs::write(
        dir.join("G Trail.bus"),
        "[scriptshare]\n\n[coupling_front]\n0\n4\n0.3\n",
    )
    .unwrap();
    std::fs::write(dir.join("Solo.bus"), "[friendlyname]\nMB\nO530\nDefault\n").unwrap();
    std::fs::write(dir.join("Solo_KI.bus"), "[model]\nx.cfg\n").unwrap();
    let main = Vehicle::load(&dir.join("G Main.bus")).unwrap();
    let trail = Vehicle::load(&dir.join("G Trail.bus")).unwrap();
    let ki = Vehicle::load(&dir.join("Solo_KI.bus")).unwrap();
    assert!(main.is_selectable() && !main.is_rear_section());
    assert!(!trail.is_selectable() && trail.is_rear_section() && trail.script_share);
    assert!(!ki.is_selectable() && !ki.is_rear_section());
    // the case of the [couple_back] name does not matter
    assert!(
        main.couple_back_path()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with("g trail.bus")
    );
    let fronts = front_sections_of(&dir.join("G Trail.bus"));
    assert_eq!(fronts.len(), 1);
    assert_eq!(fronts[0].file_name().unwrap(), "G Main.bus");
    assert!(front_sections_of(&dir.join("Solo.bus")).is_empty());
    // a rear section that carries the front's [friendlyname] is still not offered
    std::fs::write(
        dir.join("L Main.bus"),
        "[friendlyname]\nMB\nO530GL\nDefault\n\n[couple_back]\nL Trail.bus\nfalse\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("L Trail.bus"),
        "[friendlyname]\nMB\nO530GL\nDefault\n\n[coupling_front]\n0\n4\n0.3\n",
    )
    .unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let offered: Vec<String> = offered_vehicles(&files)
        .iter()
        .map(|(f, _)| f.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(offered, vec!["G Main.bus", "L Main.bus", "Solo.bus"]);
    assert_eq!(front_sections_of(&dir.join("L Trail.bus")).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An `.ovh` that leaves the registration affixes out (`[registration_automatic]` with
/// nothing but a blank line before `[model]`, as the Urumqi AI cars write it): the
/// `[model]` is a keyword, not the postfix, so the vehicle keeps its model.
#[test]
fn an_empty_registration_affix_keeps_the_next_keyword() {
    let v = Vehicle::parse(&CfgFile::from_str(
        "x.ovh",
        "[registration_free]\n\n[registration_automatic]\n\n[model]\nmodel\\model.cfg\n\n[sound]\ns.cfg\n",
    ));
    assert_eq!(v.registration_mode, 3);
    assert_eq!(v.registration_affix, (String::new(), String::new()));
    assert_eq!(v.model.as_deref(), Some("model\\model.cfg"));
    // the stock shape (prefix "B-V ", blank postfix) is unchanged
    let s = Vehicle::parse(&CfgFile::from_str(
        "y.bus",
        "[registration_automatic]\nB-V \n\n[model]\nm.cfg\n",
    ));
    assert_eq!(s.registration_affix, ("B-V ".to_string(), String::new()));
    assert_eq!(s.model.as_deref(), Some("m.cfg"));
}

/// A repaint's own `[registration_list]` followed by the template's
/// `[registration_automatic]`: the list's plate still wins (Omsi.exe 0x7e7a80 reads the
/// list file whatever the mode), prefix + number only where the list has none.
#[test]
fn list_plate_wins_over_a_later_automatic_mode() {
    let dir = std::env::temp_dir().join(format!("omsi_vehicle_regs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Nos.org"), "E1\nE2\n").unwrap();
    std::fs::write(dir.join("Regs.org"), "AB12 CDE\n").unwrap();
    std::fs::write(dir.join("x.bus"), "[number]\nNos.org\n\n[registration_list]\nRegs.org\n\n\n\n[registration_automatic]\nB-V \n\n").unwrap();
    let v = Vehicle::load(&dir.join("x.bus")).unwrap();
    assert_eq!(v.registration_mode, 3);
    assert_eq!(v.plate_of_number("E1"), "AB12 CDE");
    assert_eq!(v.plate_of_number("E2"), "B-V E2");
    // (the player's bus from the dialog: the automatic mode's plate, Edit1Change)
    assert_eq!(v.chosen_plate_of_number("E1"), "B-V E1");
    let _ = std::fs::remove_dir_all(&dir);
}
