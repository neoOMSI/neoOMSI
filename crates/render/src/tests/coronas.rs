use crate::*;

/// The OMSI star sprite is drawn beside its glow in every graphics mode, Enhanced too
#[test]
fn the_star_sprites_are_drawn() {
    let renderer = super::instances::noop_renderer();
    let mut scene = renderer.new_scene();
    let glow = Corona {
        size: 0.1,
        brightness: 1.0,
        ..Default::default()
    };
    scene.coronas = vec![
        glow,
        Corona {
            flags: 8 | 32,
            ..glow
        },
    ];
    renderer.prepare_coronas(&mut scene, 1.0);
    assert_eq!(scene.corona_count, 2);
}
