use omsi_content::{Human, TicketPack, tickets::TicketItems};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Input(PathBuf);

impl Input {
    fn new(extension: &str, text: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "neoomsi-passenger-format-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        std::fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

#[test]
fn human_geometry_voice_and_movement_parameters_survive_loading() {
    let links = (0..22)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let input = Input::new(
        "hum",
        &format!(
            "[model]\nmodel\\person.cfg\n[seatheight]\n0.82\n[humangeom]\n0.18\n1.77\n[links]\n{links}\n[voice]\nadult\n[walk_param]\n1.2\n80\n1.1\n0.9\n0.2\n[mass]\n72\n[age]\n40\n"
        ),
    );
    let human = Human::load(&input.0).unwrap();
    assert_eq!(human.model, "model\\person.cfg");
    assert_eq!(
        (human.seat_height, human.feet_dist, human.height),
        (0.82, 0.18, 1.77)
    );
    assert_eq!(human.links, (0..22).map(|n| n as f32).collect::<Vec<_>>());
    assert_eq!(human.voice, "adult");
    assert_eq!(human.walk_param, [1.2, 80.0, 1.1, 0.9, 0.2]);
    assert_eq!((human.mass, human.age), (72.0, Some(40)));
}

#[test]
fn omitted_human_walk_parameters_keep_the_default_pace() {
    let input = Input::new("hum", "[model]\nperson.cfg\n");
    let human = Human::load(&input.0).unwrap();
    assert_eq!(human.walk_param, [1.4, 66.0, 1.0, 1.0, 0.0]);
    assert_eq!(human.age, None);
}

#[test]
fn legacy_and_extended_tickets_keep_fare_age_time_and_probability_semantics() {
    let input = Input::new(
        "otp",
        "[ticketpack]\n0.4\n0.6\n0.2\n0.3\n[voicepath]\nvoices\\city\n[ticket]\nSingle\nSingle English\n6\n18\n99\n2.5\nSINGLE\n[ticket_2]\nDay child\nDay child English\n0\n0\n17\n4.0\nDAY\n1\n0.25\n",
    );
    let pack = TicketPack::load(&input.0).unwrap();
    assert_eq!(
        (
            pack.stamper_prop,
            pack.ticketbuy_prop,
            pack.chattiness,
            pack.whinge_prop
        ),
        (0.4, 0.6, 0.2, 0.3)
    );
    assert_eq!(pack.voice_path.as_deref(), Some("voices\\city"));
    assert_eq!(pack.tickets.len(), 2);
    let single = &pack.tickets[0];
    assert_eq!(
        (
            &*single.name,
            &*single.name_english,
            &*single.display_string
        ),
        ("Single", "Single English", "SINGLE")
    );
    assert_eq!(
        (
            single.max_stations,
            single.age_min,
            single.age_max,
            single.value
        ),
        (6, 18, 99, 2.5)
    );
    assert!(!single.day_ticket);
    assert_eq!(single.probability, 1.0);
    let day = &pack.tickets[1];
    assert_eq!(
        (day.age_min, day.age_max, day.value, day.probability),
        (0, 17, 4.0, 0.25)
    );
    assert!(day.day_ticket);
}

#[test]
fn ticket_items_keep_texture_bindings_and_script_variables_in_file_order() {
    let input = Input::new(
        "cti",
        "[item]\nSingle\nfarb_ticket\nsingle.bmp\n[setvar]\nvisible\n1\n[item]\nDay\nfarb_ticket\nday.bmp\n[setvar]\nprice\n2.5\n",
    );
    let items = TicketItems::load(&input.0).unwrap();
    assert_eq!(
        items.items,
        vec![
            ("Single".into(), "farb_ticket".into(), "single.bmp".into()),
            ("Day".into(), "farb_ticket".into(), "day.bmp".into())
        ]
    );
    assert_eq!(
        items.set_vars,
        vec![("visible".into(), 1.0), ("price".into(), 2.5)]
    );
}

#[test]
fn alternate_weights_accept_only_finite_nonnegative_values() {
    for (value, expected) in [
        ("0", Some(0.0)),
        ("0.25", Some(0.25)),
        ("-1", None),
        ("NaN", None),
        ("inf", None),
    ] {
        let input = Input::new("hum", &format!("[neo_weight]\n{value}\n"));
        assert_eq!(Human::load(&input.0).unwrap().weight, expected);
    }
}
