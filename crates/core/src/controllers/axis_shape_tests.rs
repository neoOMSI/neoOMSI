use super::axis_shape;
#[test]
fn characteristics_match_omsi() {
    assert!((axis_shape(0.75, 24) - 0.625).abs() < 1e-6);
    assert!((axis_shape(0.5, 24) - 0.5).abs() < 1e-6);
    assert!((axis_shape(0.5, 8) - 0.25).abs() < 1e-6);
    assert!((axis_shape(0.5, 4) - (std::f32::consts::PI / 4.0).sin()).abs() < 1e-6);
    assert!((axis_shape(0.3, 0) - 0.3).abs() < 1e-6);
    assert_eq!(axis_shape(1.0, 2), 1.0);
}
