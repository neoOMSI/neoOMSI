//! Vehicle/trip lifecycle: reroute, release, removal, population reset and the
//! passenger-facing requests.

use super::*;

impl Traffic {

    /// Take the ids of scheduled buses whose ground was unloaded under them.
    pub fn take_removed_scheduled(&mut self) -> Vec<VehicleId> {
        std::mem::take(&mut self.removed_scheduled)
    }


    /// Carry a scheduled bus's route and stops on as new tiles load, then replan.
    pub fn extend_scheduled_route(
        &mut self,
        ci: usize,
        lanes: Vec<usize>,
        stops: Vec<(usize, f32, f32, f64, i64, f32)>,
    ) {
        let car = &mut self.cars[ci];
        car.state.route.extend(lanes);
        if let Some(b) = car.bus.as_mut() {
            b.stops
                .extend(stops.into_iter().map(StopTarget::from_tuple));
        }
        car.state.planned_next = None;
        car.state.plan_next(&self.net);
    }


    /// Hand timetable bus `ci` the next trip of its tour: its route from the lane it is on
    /// (`route[0]` is that lane, `s` where it is on it) and the trip's stops. It stays where
    /// it stands; a stop right there is served in place (its layover).
    pub fn reroute(
        &mut self,
        ci: usize,
        route: Vec<usize>,
        s: f32,
        stops: Vec<(usize, f32, f32, f64, i64, f32)>,
        layover: bool,
    ) {
        let id = self.cars[ci].id;
        // a route change drops any berth the vehicle held against the old stops
        self.services.release(id);
        self.maneuvers.release(id);
        self.population.retain_on_way(id, &route);
        let net = &self.net;
        let car = &mut self.cars[ci];
        let lane = car.state.lane;
        car.state.route = route;
        car.state.route_index = 0;
        car.state.lane = lane;
        car.state.s = s;
        car.state.change = None;
        car.state.planned_next = None;
        car.state.ahead.clear();
        car.state.plan_next(net);
        car.gone = false;
        let stops = stops
            .into_iter()
            .map(StopTarget::from_tuple)
            .collect();
        match car.bus.as_mut() {
            Some(b) => b.restart(stops, layover),
            None => {
                let mut b = BusService::new(stops);
                b.state.layover = layover;
                car.bus = Some(Box::new(b));
            }
        }
    }


    /// Let timetable bus `ci` go at the end of its trip: it drives on as other traffic and
    /// is taken off as soon as nobody sees it.
    pub fn release(&mut self, ci: usize) {
        let net = &self.net;
        let car = &mut self.cars[ci];
        let st = &mut car.state;
        st.route.clear();
        st.route_index = 0;
        st.planned_next = None;
        st.ahead.clear();
        st.plan_next(net);
        if let Some(b) = car.bus.as_mut() {
            b.restart(Vec::new(), false);
        }
        car.gone = true;
    }


    /// Take a car off the road now (the player took over its tour).
    pub fn remove_car(
        &mut self,
        world: &World,
        renderer: &Renderer,
        scene: &mut Scene,
        id: VehicleId,
    ) -> bool {
        let Some(i) = self.cars.iter().position(|c| c.id == id) else {
            return false;
        };
        let c = self.cars.swap_remove(i);
        self.junctions.release(id, Reason::Removed);
        self.services.release(id);
        self.maneuvers.release(id);
        self.population.release(id, RemovalCause::TakenOver);
        self.emit_trace(TraceEvent::Removal {
            vehicle: id,
            reason: Reason::Removed,
        });
        self.orphan_sounds.extend(c.sounds);
        for r in std::iter::once(c.render).chain(c.trailer_renders) {
            world.release_vehicle(renderer, scene, r);
        }
        true
    }


    /// Remove the current traffic population before rebuilding it for a new clock time.
    pub fn reset_population(&mut self, world: &World, renderer: &Renderer, scene: &mut Scene) {
        for c in std::mem::take(&mut self.cars) {
            self.orphan_sounds.extend(c.sounds);
            for r in std::iter::once(c.render).chain(c.trailer_renders) {
                world.release_vehicle(renderer, scene, r);
            }
        }
        for (_, mut driver) in std::mem::take(&mut self.drivers) {
            driver.hide(renderer, scene);
            self.driver_pool.push(driver);
        }
        self.dormant.clear();
        self.removed_scheduled.clear();
        self.junctions.invalidate_network();
        self.services.invalidate_network();
        self.maneuvers.invalidate_network();
        self.population.clear();
        self.stop_wishes = None;
        self.framed_spawns.clear();
        self.held_at_red = 0;
        self.initial = true;
        self.last_overtaker = None;
        self.first_turner = None;
        self.first_red = None;
        self.first_yield = None;
        self.first_passer = None;
    }


    /// Tell a scheduled bus's script who wants in or out (`PAX_Entry<i>_Req`,
    /// `PAX_Exit<i>_Req`): the stock AI door scripts open the rear doors only for a stop
    /// request, which comes from the exit requests.
    pub fn set_pax_requests(&mut self, id: VehicleId, entry: &[bool], exit: &[bool]) {
        if let Some(c) = self.cars.iter_mut().find(|c| c.id == id) {
            for (i, r) in entry.iter().enumerate() {
                c.vehicle
                    .set_var(&format!("PAX_Entry{i}_Req"), *r as i32 as f32);
            }
            for (i, r) in exit.iter().enumerate() {
                c.vehicle
                    .set_var(&format!("PAX_Exit{i}_Req"), *r as i32 as f32);
            }
        }
    }


    /// Keep a scheduled bus at its stop for at least `secs` more with the doors open:
    /// passengers are still queueing at a door or stepping in.
    /// The passengers' wishes for the timetable buses' next stops (see `stop_wishes`).
    pub fn set_stop_wishes(
        &mut self,
        alighting: hashbrown::HashSet<u64>,
        waiting: hashbrown::HashSet<i64>,
    ) {
        self.stop_wishes = Some((
            alighting.into_iter().map(VehicleId).collect(),
            waiting,
        ));
    }


    pub fn hold_boarding(&mut self, id: VehicleId, secs: f32, in_doorway: bool) {
        if let Some(c) = self.cars.iter_mut().find(|c| c.id == id) {
            if let Some(b) = c.bus.as_mut() {
                b.hold(secs, in_doorway);
            }
        }
    }

}
