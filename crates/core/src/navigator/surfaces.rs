//! Drawing Navigator 2.0's surfaces (see [`crate::navmap`]): the ground as the map really
//! lays it, layer by layer, with the kerbs along the carriageways, bridge decks over what
//! runs under them and the rails on their beds.

use super::*;
use crate::navmap::{Area, Layer, SurfaceMap};

/// How far around the panel's anchor its surfaces are built (m): beyond the farthest the
/// panel shows when zoomed out at speed.
pub(super) const SURFACE_RADIUS: f64 = 520.0;

// The carriageway is what the eye goes to: everything around it stays close to the
// panel's ground.
pub(super) const GREEN: Color = Color::rgba(27, 36, 30, 1.0);
pub(super) const FOOTWAY: Color = Color::rgba(29, 29, 33, 1.0);
pub(super) const TRACK_BED: Color = Color::rgba(35, 32, 31, 1.0);
pub(super) const CARRIAGEWAY: Color = Color::rgba(76, 76, 86, 1.0);
pub(super) const PAVED: Color = Color::rgba(60, 60, 69, 1.0);
pub(super) const KERB: Color = Color::rgba(86, 86, 97, 1.0);
/// The paint on the road: lines, arrows, stop lines and crossings as the map paints them.
pub(super) const MARK: Color = Color::rgba(168, 168, 178, 0.8);
pub(super) const BRIDGE_EDGE: Color = Color::rgba(150, 150, 162, 1.0);
pub(super) const BRIDGE_SHADOW: Color = Color::rgba(8, 8, 10, 0.55);
pub(super) const RAIL: Color = Color::rgba(112, 100, 92, 1.0);

fn fill(l: Layer) -> Color {
    match l {
        Layer::Green => GREEN,
        Layer::Footway => FOOTWAY,
        Layer::Track => TRACK_BED,
        Layer::Drivable => CARRIAGEWAY,
        Layer::Paved => PAVED,
        Layer::Marking => MARK,
    }
}

fn areas(ch: &crate::navmap::Chunk, coarse: bool) -> &[Area] {
    if coarse { &ch.coarse } else { &ch.areas }
}

/// Fill the surfaces within `radius` of `centre` into `p`, relative to `anchor`; `coarse`
/// for a map seen from far. `kerb_px` is the kerb line's width (0: none).
pub(super) fn build_surfaces(
    p: &mut Painter,
    map: &SurfaceMap,
    anchor: DVec2,
    centre: DVec2,
    radius: f64,
    coarse: bool,
    kerb_px: f32,
) {
    let chunks = map.chunks_near(centre, radius);
    let pts_of = |o: DVec2, a: &Area| -> Vec<Vec3> {
        a.verts
            .iter()
            .map(|v| Vec3::new((o.x - anchor.x) as f32 + v[0], (o.y - anchor.y) as f32 + v[1], 0.0))
            .collect()
    };
    let edge_of = |o: DVec2, e: &[[f32; 2]]| -> Vec<Vec3> {
        e.iter()
            .map(|v| Vec3::new((o.x - anchor.x) as f32 + v[0], (o.y - anchor.y) as f32 + v[1], 0.0))
            .collect()
    };
    for level in [-1i8, 0, 1] {
        if level == 1 {
            // the decks' shadow on what runs under them
            for (k, ch) in &chunks {
                let o = SurfaceMap::chunk_origin(*k);
                for a in areas(ch, coarse).iter().filter(|a| a.level == 1 && a.layer == Layer::Drivable) {
                    for e in &a.edges {
                        p.ribbon(&edge_of(o, e), 1.6, 3.0, BRIDGE_SHADOW, false);
                    }
                }
            }
        }
        for layer in Layer::ALL {
            for (k, ch) in &chunks {
                let o = SurfaceMap::chunk_origin(*k);
                for a in areas(ch, coarse)
                    .iter()
                    .filter(|a| a.level == level && a.layer == layer)
                {
                    let c = if level < 0 { fill(layer).alpha(0.45) } else { fill(layer) };
                    p.world_tris(&pts_of(o, a), &a.tris, c);
                }
            }
            if layer == Layer::Drivable && kerb_px > 0.0 {
                let c = if level > 0 { BRIDGE_EDGE } else { KERB };
                for (k, ch) in &chunks {
                    let o = SurfaceMap::chunk_origin(*k);
                    for a in areas(ch, coarse)
                        .iter()
                        .filter(|a| a.level == level && a.layer == layer)
                    {
                        for e in &a.edges {
                            p.ribbon(&edge_of(o, e), 0.12, kerb_px, c, false);
                        }
                    }
                }
            }
        }
    }
    let r2 = radius * radius;
    for r in &map.rails {
        if r.iter().all(|q| (q.truncate() - centre).length_squared() > r2) {
            continue;
        }
        let pts: Vec<Vec3> = r
            .iter()
            .map(|q| Vec3::new((q.x - anchor.x) as f32, (q.y - anchor.y) as f32, 0.0))
            .collect();
        p.ribbon(&pts, 0.3, 1.0, RAIL, false);
    }
}
