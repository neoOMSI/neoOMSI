use super::*;

fn road_spline() -> ::scenery::sli::Spline {
    use ::scenery::sli::{SplineProfile, SplineProfilePoint};
    ::scenery::sli::Spline {
        profiles: vec![SplineProfile {
            points: vec![
                SplineProfilePoint {
                    x: -4.0,
                    ..Default::default()
                },
                SplineProfilePoint {
                    x: 4.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        // This editor selection width is deliberately much wider than the mesh.
        height_profiles: vec![::scenery::sli::HeightProfile {
            x0: -40.0,
            x1: 40.0,
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn road_candidates_use_drawn_width_and_ignore_nonroad_assets() {
    let def = road_spline();
    assert!(road_sections("Splines/StreetLight.sli", &def).is_empty());
    assert!(road_surface_name("Splines/Fahrbahn.sli"));
    assert!(!road_surface_name("Splines/Straßenbahn.sli"));
    assert_eq!(
        road_sections("Splines/Str_2spur_6m.sli", &def),
        vec![(-4.0, 4.0, 0.0)]
    );
    let mut only_editor = def.clone();
    only_editor.only_editor = true;
    assert!(road_sections("Splines/Str_2spur_6m.sli", &only_editor).is_empty());
    let mut asphalt = def;
    asphalt.textures.push(::scenery::sli::SplineTexture {
        file: "Texture/asphalt_rough.bmp".into(),
        ..Default::default()
    });
    assert_eq!(
        road_sections("Splines/CustomCurve.sli", &asphalt),
        vec![(-4.0, 4.0, 0.0)]
    );
    assert!(road_sections("Splines/BS_Gehweg_Asphalt01_MD_04m.sli", &asphalt).is_empty());
}

#[test]
fn road_surface_bounds_keep_offsets_medians_and_unused_textures() {
    use ::scenery::sli::{Spline, SplineProfile, SplineProfilePoint, SplineTexture};
    let profile = |texture, lo, hi| SplineProfile {
        texture,
        points: vec![
            SplineProfilePoint {
                x: lo,
                ..Default::default()
            },
            SplineProfilePoint {
                x: hi,
                ..Default::default()
            },
        ],
    };
    let mut def = Spline {
        textures: vec![
            SplineTexture {
                file: "str_asphdrk.bmp".into(),
                ..Default::default()
            },
            SplineTexture {
                file: "str_side1.bmp".into(),
                ..Default::default()
            },
            SplineTexture {
                file: "gras1.bmp".into(),
                ..Default::default()
            },
        ],
        profiles: vec![
            profile(1, -25.0, -18.0),
            profile(0, -18.0, -15.0),
            profile(0, -15.0, -12.0),
            profile(2, -12.0, 12.0),
            profile(0, 12.0, 18.0),
            profile(1, 18.0, 25.0),
        ],
        ..Default::default()
    };
    assert_eq!(
        road_sections("Custom/divided.sli", &def),
        vec![(-18.0, -12.0, 0.0), (12.0, 18.0, 0.0)]
    );
    def.profiles = vec![profile(1, -25.0, 25.0)];
    assert!(
        road_sections("Custom/surface.sli", &def).is_empty(),
        "unused asphalt texture must not turn pavement into a road"
    );
    def.profiles = vec![profile(0, -4.0, 4.0)];
    def.paths.push(::scenery::PathDef {
        kind: 2,
        ..Default::default()
    });
    assert!(road_sections("Custom/track.sli", &def).is_empty());
}

#[test]
fn carriageways_take_the_paths_widened_to_the_road_surface() {
    use ::scenery::PathDef;
    let mut def = road_spline();
    def.textures.push(::scenery::sli::SplineTexture {
        file: "Texture/asphalt.bmp".into(),
        ..Default::default()
    });
    let path = |x: f32| PathDef {
        kind: 0,
        start: [x, 0.0, 0.0],
        width: 3.0,
        ..Default::default()
    };
    // two paths in the middle of an 8 m surface: one carriageway, kerb to kerb
    def.paths = vec![path(-1.75), path(1.75)];
    let spans = carriageway_sections("Splines/Road.sli", &def, false);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert_eq!((spans[0].0, spans[0].1), (-4.0, 4.0));
    assert!(spans[0].3);
    // only one path, on the right half: still the whole surface it lies on
    def.paths = vec![path(2.0)];
    let spans = carriageway_sections("Splines/Road.sli", &def, true);
    assert_eq!((spans[0].0, spans[0].1), (-4.0, 4.0), "{spans:?}");
    // no path: the surface alone, marked as such
    def.paths.clear();
    let spans = carriageway_sections("Splines/Road.sli", &def, false);
    assert_eq!(spans, vec![(-4.0, 4.0, 0.0, false)]);
    // an invisible traffic spline: its paths show through the lanes instead
    def.paths = vec![path(0.0)];
    def.only_editor = true;
    assert!(carriageway_sections("Splines/Road.sli", &def, false).is_empty());
}
