use super::*;

fn change_result(
    owed: f32,
    given: f32,
    tolerance: f32,
    needed: usize,
    count: usize,
    draw: f32,
) -> (bool, bool, bool) {
    let too_much = tolerance < given - owed;
    let enough = owed - given <= tolerance;
    let many = needed as f32 * (draw + 1.5) <= count as f32 && count > 0;
    (enough && !too_much, too_much, many)
}

impl Humans {
    /// Cancel UI/accounting owned by this passenger without taking physical tray money.
    pub(in crate::humans) fn cancel_fare(&mut self, owner: u32) {
        if self.desk.desk_busy != Some(owner) {
            return;
        }
        self.desk.desk_busy = None;
        self.desk.pardons = 0;
        self.desk.pardon_max = 0;
        self.request = None;
        self.paid = None;
        self.change_due = None;
    }
    /// A ticket of the pack for a passenger of `age`: those whose age
    /// range holds it, weighted by their probability - a day ticket's by the time of day
    /// as well (`day_ticket_factor`). `max_stations` plays no part in the choice.
    pub(in crate::humans) fn pick_ticket(&mut self, age: f32) -> Option<usize> {
        let r = self.rand_f() as f32;
        let day = day_ticket_factor(self.time_of_day);
        let t = self.tickets.as_ref()?;
        let weight = |tk: &::content::tickets::Ticket| {
            if (tk.age_min as f32) > age || (tk.age_max as f32) < age {
                0.0
            } else if tk.day_ticket {
                tk.probability.max(0.0) * day
            } else {
                tk.probability.max(0.0)
            }
        };
        let total: f32 = t.tickets.iter().map(weight).sum();
        if total <= 0.0 {
            return None;
        }
        let mut x = r * total;
        for (i, tk) in t.tickets.iter().enumerate() {
            let w = weight(tk);
            if w > 0.0 && x < w {
                return Some(i);
            }
            x -= w;
        }
        None
    }

    /// OMSI's `change_take`: the driver takes back the coins lying on the change tray.
    pub fn take_change_tray(&mut self) {
        if let Some(m) = self.money.as_mut() {
            m.clear(true);
        }
    }
}

impl Humans {
    /// sub_5ce4e0: stamp (stamper_prop) or buy (ticketbuy_prop) at a bus that has a
    /// validator / a cash desk, else nothing to do; the ticket bought (sub_5ce2dc).
    pub(in crate::humans) fn decide_pax_ticket(
        &mut self,
        i: usize,
        bn: &BusNow,
    ) -> (TicketAction, u8) {
        let Some(tp) = self.tickets.clone() else {
            return (TicketAction::None, 0);
        };
        let mut r = self.rand_f() as f32;
        if bn.cabin.stamper.is_some() {
            if r < tp.stamper_prop {
                return (TicketAction::Stamp, 0);
            }
            r -= tp.stamper_prop;
        }
        // (the sale also needs the option on, `boarding` not "walk")
        if bn.cabin.sale.is_some()
            && r < tp.ticketbuy_prop
            && !self.boarding.eq_ignore_ascii_case("walk")
        {
            let age = self.people[i].age;
            if let Some(t) = self.pick_ticket(age) {
                return (TicketAction::Buy, (t + 1).min(255) as u8);
            }
            return (TicketAction::Buy, 0);
        }
        (TicketAction::None, 0)
    }

    /// Task 4 (case 4): the validator, the cash desk, and on to the place.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::humans) fn task_to_place(
        &mut self,
        i: usize,
        buses: &[BusNow],
        bus_ix: &HashMap<BusId, usize>,
        world: &World,
        given_ticket: Option<f32>,
        place_payment: &mut dyn FnMut(&mut crate::money::Money, &[usize], Vec3, [f32; 2]),
        taken_ticket: &mut bool,
    ) {
        let p = self.pax(i).unwrap().clone();
        let Some(bn) = p.inside.and_then(|b| bus_ix.get(&b).map(|k| &buses[*k])) else {
            return;
        };
        // the validator's path point can lie past it (the Citaro's)
        if p.movement == Movement::AlongPath
            && p.ticket == TicketAction::Stamp
            && p.fare_phase == FarePhase::None
            && bn.id == BusId::Player
        {
            if let (Some((_, dev)), Some(q)) = (
                bn.cabin.stamper,
                p.pt.and_then(|k| bn.cabin.graph.points.get(k)),
            ) {
                let dev = dev.as_dvec3().truncate();
                let here = (dev - p.pos.truncate()).length();
                if here < 1.5 && (dev - q.as_dvec3().truncate()).length() > here + 0.05 {
                    self.pax_mut(i).unwrap().movement = Movement::AtPathEnd;
                }
            }
        }
        let p = self.pax(i).unwrap().clone();
        if p.movement == Movement::AtPathEnd {
            if p.ticket < TicketAction::Stamp {
                self.set_task(i, Task::SittingInBus, buses, bus_ix, world);
                return;
            }
            if bn.id != BusId::Player {
                let pp = self.pax_mut(i).unwrap();
                pp.fare_phase = FarePhase::None;
                pp.ticket = TicketAction::None;
                self.route_to_place(i, bn);
                return;
            } else {
                let stand = bn.cabin.stamper.and_then(|(_, dev)| {
                    let from = p.pos;
                    let ahead = (dev.as_dvec3() - from).truncate().normalize_or_zero();
                    let spot = self.people[i]
                        .ty
                        .rig
                        .reach_spot((dev.z as f64 - from.z) as f32)
                        .as_dvec2();
                    let right = glam::DVec2::new(ahead.y, -ahead.x);
                    let at = dev.as_dvec3().truncate() - ahead * spot.y - right * spot.x;
                    ((at - from.truncate()).dot(ahead) > 0.08).then(|| at.extend(from.z))
                });
                let pp = self.pax_mut(i).unwrap();
                pp.movement = Movement::Turning;
                pp.smooth = true;
                if pp.ticket == TicketAction::Stamp {
                    if let Some((_, dev)) = bn.cabin.stamper {
                        pp.target = dev.as_dvec3();
                        pp.target_bus = true;
                        pp.reach_at = dev;
                    }
                    pp.fare_phase = FarePhase::Validating;
                    match stand {
                        Some(at) => {
                            pp.target = at;
                            pp.movement = Movement::ToTarget;
                            pp.short = false;
                            pp.timer = 30.0;
                            pp.reach = false;
                        }
                        None => {
                            pp.timer = STAMP_TIME;
                            pp.reach = true;
                        }
                    }
                } else {
                    pp.fare_phase = FarePhase::RequestTicket;
                    if let Some(m) = bn.cabin.money_point {
                        pp.target = m.as_dvec3();
                        pp.target_bus = true;
                        pp.reach_at = m;
                    }
                }
            }
        }
        let p = self.pax(i).unwrap().clone();
        if p.ticket == TicketAction::Stamp {
            if p.fare_phase == FarePhase::Validating
                && !p.reach
                && p.movement == Movement::AtTarget
            {
                let pp = self.pax_mut(i).unwrap();
                pp.movement = Movement::Turning;
                pp.target = pp.reach_at.as_dvec3();
                pp.target_bus = true;
                pp.reach = true;
                pp.timer = STAMP_TIME;
            } else if p.fare_phase == FarePhase::Validating && !p.reach && p.timer <= 0.0 {
                self.route_to_place(i, bn);
                let pp = self.pax_mut(i).unwrap();
                pp.pt = bn.cabin.stamper.and_then(|s| s.0);
                pp.ticket = TicketAction::None;
                pp.fare_phase = FarePhase::None;
            } else if p.fare_phase == FarePhase::Validating && p.reach && p.timer < STAMP_RELEASE {
                // the validator stamps
                self.stamped.push(bn.id);
                let pp = self.pax_mut(i).unwrap();
                pp.fare_phase = FarePhase::Validated;
                pp.reach = false;
            } else if p.fare_phase == FarePhase::Validated && p.timer <= 0.0 {
                self.route_to_place(i, bn);
                let pp = self.pax_mut(i).unwrap();
                pp.pt = bn.cabin.stamper.and_then(|s| s.0);
                pp.ticket = TicketAction::None;
                pp.fare_phase = FarePhase::None;
            }
        } else if p.ticket == TicketAction::Buy {
            self.desk_sale(i, bn, given_ticket, place_payment, taken_ticket);
        }
    }

    /// The ticket sale at the player's cash desk (case 4 with +0x61c = 3, 0x62c780 -
    /// 0x62d104): the ticket asked for, the money on the desk, the ticket taken, the change
    /// counted. The passenger asks again every 5 s (3 s after the second time) until the
    /// driver gets it right; the counter of those requests (`pardons`, the original's
    /// global at 0x859bc4) is shared by everybody at the desk.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::humans) fn desk_sale(
        &mut self,
        i: usize,
        bn: &BusNow,
        given_ticket: Option<f32>,
        place_payment: &mut dyn FnMut(&mut crate::money::Money, &[usize], Vec3, [f32; 2]),
        taken_ticket: &mut bool,
    ) {
        let p = self.pax(i).unwrap().clone();
        // the game plays the driver in `auto` boarding (a setting; OMSI has no such mode)
        let auto = self.boarding.eq_ignore_ascii_case("auto");
        let id = p.ticket_id as usize;
        let (name, value) = self
            .tickets
            .as_ref()
            .and_then(|t| t.tickets.get(id.saturating_sub(1)))
            .map(|t| (t.name.clone(), t.value))
            .unwrap_or_default();
        let tol = self
            .money
            .as_ref()
            .map(|m| m.smallest_value())
            .unwrap_or(0.01)
            / 2.0;
        let owed = p.paid - p.price;
        // the change on the tray (sub_7e8900): enough of it, too much, too many coins
        let change = |h: &mut Self| -> (bool, bool, bool) {
            if auto {
                return (true, false, false);
            }
            let given = h.money.as_ref().map(|m| m.change_value()).unwrap_or(0.0);
            let needed = h
                .money
                .as_mut()
                .map(|m| m.exact_coins_for(owed.max(0.0)).len())
                .unwrap_or(0);
            let count = h.money.as_ref().map(|m| m.change_count()).unwrap_or(0);
            let r = h.rand_f() as f32;
            change_result(owed, given, tol, needed, count, r)
        };
        // the ticket given (sub_7e8f14): right, or a wrong one
        let ticket = |h: &Self| -> (bool, bool) {
            if auto || h.give_ticket {
                return (true, false);
            }
            let given = given_ticket.unwrap_or(-1.0);
            if given < 0.0 {
                return (false, false);
            }
            let ok = (given - (id as f32 - 1.0)).abs() < 0.5;
            (ok, !ok)
        };
        let (ch_ok, too_much, many) = if p.fare_phase == FarePhase::AwaitChange {
            change(self)
        } else {
            (false, false, false)
        };
        let (tk_ok, wrong) = if p.fare_phase == FarePhase::AwaitTicket {
            ticket(self)
        } else {
            (false, false)
        };
        if p.fare_phase == FarePhase::RequestTicket {
            // the desk free (sub_7d1fec, +0x7a8): "Einmal ..., bitte"
            if self.desk.desk_busy.is_some_and(|d| d != self.people[i].id) {
                return;
            }
            let k = 1 + self.rand() % 2;
            self.say_ex(i, &format!("Ticket_{id}_{k}"), false);
            self.ticket_requests += 1;
            self.request = Some((name, value));
            self.desk.desk_busy = Some(self.people[i].id);
            self.desk.pardons = 0;
            self.desk.pardon_max = 0;
            let pp = self.pax_mut(i).unwrap();
            pp.talking = true;
            pp.timer = HAND_TIME;
            pp.reach = true;
            pp.fare_phase = FarePhase::Paying;
            pp.paid = 0.0;
        } else if p.fare_phase == FarePhase::Paying {
            if p.timer > 0.0 {
                return;
            }
            // the money on the desk (sub_7e8254)
            let point = bn.cabin.money_var;
            let mut paid = value;
            if let Some(m) = self.money.as_mut() {
                let coins = if self.exact_fare || auto {
                    m.exact_coins_for(value)
                } else {
                    m.omsi_coins_for(value)
                };
                paid = m.value_of(&coins);
                if let Some((pos, var)) = point {
                    place_payment(m, &coins, pos, var);
                }
            }
            self.paid = Some((paid, value));
            let pp = self.pax_mut(i).unwrap();
            pp.paid = paid;
            pp.fare_phase = FarePhase::AwaitTicket;
            pp.reach = false;
            pp.talking = false;
            pp.look_driver = false;
            pp.timer = 10.0;
        } else if p.fare_phase == FarePhase::AwaitTicket && tk_ok {
            // the ticket: the hand to where it comes out
            let pp = self.pax_mut(i).unwrap();
            pp.fare_phase = FarePhase::TakingTicket;
            pp.timer = HAND_TIME;
            pp.reach = true;
            if let Some((_, t)) = bn.cabin.sale {
                pp.reach_at = t;
            }
            self.desk.pardons = 0;
        } else if p.fare_phase == FarePhase::TakingTicket && p.timer <= 0.0 {
            self.pax_mut(i).unwrap().reach = false;
            self.tickets_sold += 1;
            self.ticket_cash += value;
            *taken_ticket = true;
            if let Some(m) = self.money.as_mut() {
                m.clear(false);
            }
            self.paid = None;
            let pp = self.pax_mut(i).unwrap();
            if (p.price - p.paid).abs() > tol && !auto {
                pp.fare_phase = FarePhase::AwaitChange;
                pp.timer = 5.0;
                self.change_due = Some(owed);
            } else {
                pp.fare_phase = FarePhase::TakingChange;
                self.change_due = Some(0.0);
            }
            let pp = self.pax_mut(i).unwrap();
            pp.talking = false;
            pp.look_driver = false;
        } else if p.fare_phase == FarePhase::AwaitChange
            && (ch_ok || (self.desk.pardons > 1 && too_much))
        {
            let pp = self.pax_mut(i).unwrap();
            pp.fare_phase = FarePhase::TakingChange;
            pp.timer = HAND_TIME;
            pp.reach = true;
            pp.bad_change = many;
            if let Some(c) = bn.cabin.change_point {
                pp.reach_at = c;
            }
            if many {
                self.say_ex(i, "BadChange_1", true);
                self.ticket_points += 1;
            } else {
                if !too_much {
                    self.ticket_points += 2;
                }
                let r = self.rand_f() as f32;
                if !too_much && self.tickets.as_ref().is_some_and(|t| r < t.chattiness) {
                    self.say_ex(i, "Thanks_1", false);
                }
            }
        } else if p.fare_phase == FarePhase::TakingChange && p.timer <= 0.0 {
            if let Some(m) = self.money.as_mut() {
                m.clear(true);
            }
            self.cancel_fare(self.people[i].id);
            self.boarded += 1;
            self.served += 1;
            self.route_to_place(i, bn);
            let pp = self.pax_mut(i).unwrap();
            pp.reach = false;
            pp.talking = false;
            pp.look_driver = false;
            pp.fare_phase = FarePhase::None;
            pp.ticket = TicketAction::None;
            pp.pt = bn.cabin.sale.and_then(|s| s.0);
        } else if (p.fare_phase == FarePhase::AwaitTicket || p.fare_phase == FarePhase::AwaitChange)
            && (p.timer <= 0.0 || (too_much && self.desk.pardons == 0))
        {
            // asking again (0x62ce80)
            let n = self.desk.pardons as u64;
            let r = self.rand() % (n + 2);
            self.pax_mut(i).unwrap().timer = if n < 2 { 5.0 } else { 3.0 };
            let line = if (n == 0 || r == 0) && p.fare_phase == FarePhase::AwaitTicket {
                if wrong {
                    "BadTicket_A".to_string()
                } else {
                    "PardonTicket_1".to_string()
                }
            } else if wrong && n == 1 && p.fare_phase == FarePhase::AwaitTicket {
                "BadTicket_B".to_string()
            } else if !too_much && n == 0 && p.fare_phase == FarePhase::AwaitChange {
                "TooFew_A".to_string()
            } else if !too_much && n == 1 && p.fare_phase == FarePhase::AwaitChange {
                "TooFew_B".to_string()
            } else if too_much && n == 0 && p.fare_phase == FarePhase::AwaitChange {
                "TooMuch_A".to_string()
            } else if too_much && n == 1 && p.fare_phase == FarePhase::AwaitChange {
                self.desk.pardons = 2;
                "TooMuch_B".to_string()
            } else {
                format!("Pardon_{}", r.min(3))
            };
            self.say_ex(i, &line, true);
            self.pax_mut(i).unwrap().talking = true;
            let skip = self.desk.pardons != 0 && self.rand_f() >= 0.7;
            if !skip {
                self.desk.pardons = self.desk.pardons.saturating_add(1);
            }
        }
        self.desk.pardon_max = self.desk.pardon_max.max(self.desk.pardons);
    }
}

pub(in crate::humans) struct FareDesk {
    /// Who is at the player's cash desk (+0x7a8), how often the driver has been asked
    /// again (0x859bc4) and the most of that in this sale (0x859df4).
    pub(in crate::humans) desk_busy: Option<u32>,
    pub(in crate::humans) pardons: u8,
    pub(in crate::humans) pardon_max: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_acceptance_uses_half_the_smallest_coin_and_rates_excess_pieces() {
        assert_eq!(
            change_result(1.0, 1.0, 0.005, 1, 1, 0.5),
            (true, false, false)
        );
        assert_eq!(
            change_result(1.0, 0.99, 0.005, 1, 1, 0.5),
            (false, false, false)
        );
        assert_eq!(
            change_result(1.0, 1.01, 0.005, 1, 1, 0.5),
            (false, true, false)
        );
        assert_eq!(
            change_result(1.0, 1.0, 0.005, 1, 2, 0.5),
            (true, false, true)
        );
        assert_eq!(
            change_result(0.0, 0.0, 0.005, 0, 0, 0.5),
            (true, false, false)
        );
    }
}

const STAMP_TIME: f32 = 2.0;
const STAMP_RELEASE: f32 = 0.6;
const HAND_TIME: f32 = 1.4;
