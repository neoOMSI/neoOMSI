use super::*;

#[derive(Debug, Default)]
pub struct LoadStats {
    pub tiles: usize,
    pub objects: usize,
    pub trees: usize,
    pub splines: usize,
    pub object_types: usize,
    pub spline_types: usize,
    pub textures: usize,
    /// Object records whose type is not in this installation (logged once per file).
    pub failed_objects: usize,
    /// Parking spaces the map leaves empty on purpose (a quarter of them).
    pub empty_spaces: usize,
    /// `[splineAttachement]` rows (and repeaters) that put objects on a spline.
    pub rows: usize,
    /// `[attachObj]` records, and those whose parent or attachment point is missing.
    pub attached: usize,
    pub unattached: usize,
    /// Objects stood on the ground (trees and invisible helpers included).
    pub objects_placed: usize,
    /// Ground points pulled onto `[spline_terrain_align]` roads, on how many tiles, and the
    /// biggest move (metres, where).
    pub ground_aligned: usize,
    pub ground_aligned_tiles: usize,
    pub ground_moved_most: Option<(f32, f64, f64)>,
    /// Tiles with terrain deformation, and placed definitions with local height deformation.
    pub ground_deformed_tiles: usize,
    pub crossings_warped: usize,
}

impl LoadStats {
    /// What the roads and crossings did to the ground.
    pub fn log_ground(&self) {
        if self.ground_aligned > 0 {
            log::info!(
                "terrain aligned to the roads: {} ground points on {} tiles",
                self.ground_aligned,
                self.ground_aligned_tiles
            );
            if let Some((d, x, y)) = self.ground_moved_most {
                log::info!("  the ground moved most at ({x:.0}, {y:.0}): {d:.2} m");
            }
        }
        if self.ground_deformed_tiles > 0 || self.crossings_warped > 0 {
            log::info!(
                "crossings deform the terrain on {} tiles; {} crossings use a local height field",
                self.ground_deformed_tiles,
                self.crossings_warped
            );
        }
    }

    /// Add what a batch of prepared tiles counted.
    pub fn add_prepared(&mut self, s: &LoadStats) {
        self.failed_objects += s.failed_objects;
        self.empty_spaces += s.empty_spaces;
        self.rows += s.rows;
        self.attached += s.attached;
        self.unattached += s.unattached;
        self.objects_placed += s.objects_placed;
        self.ground_aligned += s.ground_aligned;
        self.ground_aligned_tiles += s.ground_aligned_tiles;
        self.ground_deformed_tiles += s.ground_deformed_tiles;
        self.crossings_warped += s.crossings_warped;
        if let Some(b) = s.ground_moved_most {
            if self.ground_moved_most.map(|m| b.0 > m.0).unwrap_or(true) {
                self.ground_moved_most = Some(b);
            }
        }
    }
}
