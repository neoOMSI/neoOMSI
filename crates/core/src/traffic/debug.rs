//! Read-only, bounded debug snapshots. Built only when the developer window requests them.
use super::*;

pub(crate) struct TrafficDebugCar {
    pub id: u64,
    pub model: String,
    pub position: DVec3,
    pub boxes: Vec<Obb>,
    pub path: Vec<DVec3>,
    pub blocker: Option<(u64, DVec3)>,
    pub constraint: Option<DVec3>,
    pub stop: Option<(i64, DVec3, f32)>,
    pub detail: String,
    pub label: String,
    pub speed: f32,
    pub changing: bool,
}
pub(crate) struct TrafficDebugFrame {
    pub time: f32,
    pub active: usize,
    pub dormant: usize,
    pub timings: [f64; 3],
    pub cars: Vec<TrafficDebugCar>,
}
impl Traffic {
    pub(crate) fn debug_frame(&self, at: DVec3, radius: f64, selected: u64) -> TrafficDebugFrame {
        let mut near: Vec<_> = self
            .cars
            .iter()
            .filter(|c| {
                (selected != 0 && selected == c.id.get())
                    || (selected == 0
                        && (c.vehicle.position - at).length_squared() <= radius * radius)
            })
            .collect();
        near.sort_by(|a, b| {
            (a.vehicle.position - at)
                .length_squared()
                .total_cmp(&(b.vehicle.position - at).length_squared())
        });
        let cars = near.into_iter().take(64).map(|c| {
            let st = &c.state;
            let mut boxes = vec![safety::road_body_box(&c.vehicle, &c.body, &c.caps)];
            boxes.extend(c.vehicle.trailers.iter().filter_map(|t|
                t.ty.def.bounding_box.map(|bb| Obb::from_box(bb, t.position, t.heading))));
            let path = (0..=20).map(|k| st.way_point(&self.net, k as f32 * 3.0) + DVec3::Z * 0.25).collect();
            // Tick-tail removals can invalidate the per-tick index before UI collection.
            let blocker = c.lead_car.or(c.geo_block).and_then(|id| self.cars.iter()
                .find(|other| other.id == id).map(|other|
                    (id.get(), other.vehicle.position + DVec3::Z * 1.0)));
            let constraint = (!c.why.0.is_none()).then(|| st.way_point(&self.net, st.front + c.why.1) + DVec3::Z * 0.3);
            let stop = c.bus.as_ref().and_then(|b| b.stops.front()).and_then(|t| {
                let lane = *st.route.get(t.route_index)?;
                let (p, h) = self.net.lanes.get(lane)?.at(t.s);
                let h = (h as f64).to_radians();
                Some((t.stop.get(), p + DVec3::new(h.cos(), -h.sin(), 0.0) * t.bay as f64 + DVec3::Z * 0.4,
                    st.route_distance(&self.net, t.route_index, t.s)))
            });
            TrafficDebugCar {
                id: c.id.get(), model: c.vehicle.ty.def.type_name.clone(), position: c.vehicle.position,
                boxes, path, blocker, constraint, stop, speed: st.speed, changing: st.change.is_some() || c.maneuver.passing.is_some(),
                label: format!("#{} {:.0} km/h | lane {}\n{} | {:?}", c.id.get(), st.speed * 3.6, st.lane,
                    if c.why.0.is_none() { "clear" } else { c.why.0.label() }, c.bus.as_ref().map(|b| b.state.phase)),
                detail: format!("lane {} s {:.1}, {} gap {:.1}; maneuver {:?}; service {:?}; ground {}; fault {:?}",
                    st.lane, st.s, if c.why.0.is_none() { "clear" } else { c.why.0.label() }, c.why.1,
                    if st.change.is_some() { ManeuverPhase::LaneChange } else if c.maneuver.passing.is_some() { ManeuverPhase::Passing }
                    else if c.maneuver.park.is_some() { ManeuverPhase::Parking } else { ManeuverPhase::Idle },
                    c.bus.as_ref().map(|b| b.state.phase), c.body.ground_supported, c.motion_fault),
            }
        }).collect();
        TrafficDebugFrame {
            time: self.time,
            active: self.cars.len(),
            dormant: self.dormant_count(),
            timings: self.tick_split,
            cars,
        }
    }
}
