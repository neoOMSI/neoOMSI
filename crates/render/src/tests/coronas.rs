use crate::*;

/// Enhanced draws no OMSI star sprites (#365); Vanilla keeps them.
#[test]
fn enhanced_leaves_out_the_star_sprites() {
    let renderer = super::instances::noop_renderer();
    let mut scene = renderer.new_scene();
    let glow = Corona {
        size: 0.1,
        brightness: 1.0,
        ..Default::default()
    };
    scene.coronas = vec![glow, Corona { flags: 8, ..glow }];
    renderer.prepare_coronas(&mut scene, 1.0, false);
    assert_eq!(scene.corona_count, 2);
    renderer.prepare_coronas(&mut scene, 1.0, true);
    assert_eq!(scene.corona_count, 1);
}
