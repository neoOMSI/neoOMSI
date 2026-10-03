//! `passengercabin.cfg` (unit `mc_passcabin`).

use omsi_cfg::CfgFile;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PassPos {
    pub pos: [f32; 3],
    pub height: f32,
    pub rot: f32,
    /// The four `[interiorlight]`s (by index, -1 none) that light a person on this seat, as
    /// Omsi.exe keeps them in the seat record (+0x24..+0x27, 0x5ce680): the first seat of
    /// the file, `[passpos]` or `[drivpos]`, has 0 1 2 3, every later one those of the seat
    /// before it, and an `[illumination_interior]` sets those of the seat written last.
    pub illumination: [i32; 4],
    /// Where it stands in Omsi.exe's one list of `[passpos]` and `[drivpos]` (file order):
    /// the seat number scripts ask `GetHumanCountOnSeat` about (0x7d39a4) - with the
    /// driver's place first, as most cabins have it, the first `[passpos]` is seat 1.
    pub file_index: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    pub path_point: i32,
    pub no_ticket_sale: bool,
    pub with_button: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Point3 {
    pub path_point: i32,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VarPoint {
    pub pos: [f32; 3],
    pub var: [f32; 2],
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PassengerCabin {
    pub entries: Vec<Entry>,
    pub exits: Vec<i32>,
    pub link_to_next_veh: Option<i32>,
    pub link_to_prev_veh: Option<i32>,
    pub stampers: Vec<Point3>,
    pub ticket_sales: Vec<Point3>,
    pub money_points: Vec<VarPoint>,
    pub change_points: Vec<VarPoint>,
    pub pass_positions: Vec<PassPos>,
    pub driver_positions: Vec<PassPos>,
}

impl PassengerCabin {
    pub fn load(path: &Path) -> Result<PassengerCabin, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    pub fn parse(f: &CfgFile) -> PassengerCabin {
        let mut c = PassengerCabin::default();
        let mut r = f.reader().disabled_blocks();
        // the seat written last (driver's or not, and which): Omsi.exe keeps both kinds in
        // one list, in the order of the file
        let mut last: Option<(bool, usize)> = None;
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "entry" => {
                    let mut e = Entry {
                        path_point: r.i32(),
                        ..Default::default()
                    };
                    loop {
                        let save = r.pos();
                        let w = r.word().to_ascii_lowercase();
                        match w.as_str() {
                            "{noticketsale}" => e.no_ticket_sale = true,
                            "{withbutton}" => e.with_button = true,
                            _ => {
                                r.seek(save);
                                break;
                            }
                        }
                    }
                    c.entries.push(e);
                }
                "exit" => c.exits.push(r.i32()),
                "linktonextveh" => c.link_to_next_veh = Some(r.i32()),
                "linktoprevveh" => c.link_to_prev_veh = Some(r.i32()),
                "stamper" => c.stampers.push(Point3 {
                    path_point: r.i32(),
                    pos: r.f32s::<3>(),
                }),
                "ticket_sale" => c.ticket_sales.push(Point3 {
                    path_point: r.i32(),
                    pos: r.f32s::<3>(),
                }),
                "ticket_sale_money_point"
                | "ticket_sale_money_point_2"
                | "ticket_sale_change_point"
                | "ticket_sale_change_point_2" => {
                    let pos = r.f32s::<3>();
                    let var = r.f32s::<2>();
                    let parent = if k.ends_with("_2") {
                        Some(r.str().to_string())
                    } else {
                        None
                    };
                    let p = VarPoint { pos, var, parent };
                    if k.contains("money") {
                        c.money_points.push(p);
                    } else {
                        c.change_points.push(p);
                    }
                }
                "passpos" | "drivpos" => {
                    let pos = r.f32s::<3>();
                    let height = r.f32();
                    let rot = r.f32();
                    let illumination = match last {
                        Some((true, i)) => c.driver_positions[i].illumination,
                        Some((false, i)) => c.pass_positions[i].illumination,
                        None => [0, 1, 2, 3],
                    };
                    let file_index = c.pass_positions.len() + c.driver_positions.len();
                    let p = PassPos {
                        pos,
                        height,
                        rot,
                        illumination,
                        file_index,
                    };
                    if k == "passpos" {
                        c.pass_positions.push(p);
                        last = Some((false, c.pass_positions.len() - 1));
                    } else {
                        c.driver_positions.push(p);
                        last = Some((true, c.driver_positions.len() - 1));
                    }
                }
                "illumination_interior" => {
                    // (exactly four lines, each a signed byte in the record)
                    let l = [r.i32(), r.i32(), r.i32(), r.i32()];
                    let seat = match last {
                        Some((true, i)) => Some(&mut c.driver_positions[i]),
                        Some((false, i)) => Some(&mut c.pass_positions[i]),
                        None => None,
                    };
                    if let Some(seat) = seat {
                        seat.illumination = l;
                    }
                }
                _ => {}
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_lamps_follow_the_seat_before() {
        let text = "[drivpos]\n-0.8\n4.5\n1.0\n0.5\n0\n\n[illumination_interior]\n4\n5\n-1\n-1\n\n\
                    [passpos]\n0.5\n2\n1\n0.5\n0\n\n[passpos]\n0.5\n1\n1\n0.5\n0\n\n\
                    [illumination_interior]\n6\n7\n8\n9\n\n[passpos]\n0.5\n0\n1\n0.5\n0\n";
        let c = PassengerCabin::parse(&CfgFile::from_str("passengercabin.cfg", text));
        assert_eq!(c.driver_positions[0].illumination, [4, 5, -1, -1]);
        assert_eq!(c.pass_positions[0].illumination, [4, 5, -1, -1]);
        assert_eq!(c.pass_positions[1].illumination, [6, 7, 8, 9]);
        assert_eq!(c.pass_positions[2].illumination, [6, 7, 8, 9]);
        let c = PassengerCabin::parse(&CfgFile::from_str(
            "passengercabin.cfg",
            "[passpos]\n0\n0\n1\n0.5\n0\n",
        ));
        assert_eq!(c.pass_positions[0].illumination, [0, 1, 2, 3]);
    }

    #[test]
    fn seats_are_numbered_with_the_drivers_place() {
        let text = "[drivpos]\n-0.8\n4.5\n1.0\n0.5\n0\n\n[passpos]\n0.5\n2\n1\n0.5\n0\n\n\
                    [passpos]\n0.5\n1\n1\n0.5\n0\n\n[drivpos]\n0.8\n4.5\n1.0\n0.5\n0\n\n[passpos]\n0.5\n0\n1\n0.5\n0\n";
        let c = PassengerCabin::parse(&CfgFile::from_str("passengercabin.cfg", text));
        let seats: Vec<usize> = c.pass_positions.iter().map(|p| p.file_index).collect();
        assert_eq!(seats, [1, 2, 4]);
        assert_eq!(
            c.driver_positions
                .iter()
                .map(|p| p.file_index)
                .collect::<Vec<_>>(),
            [0, 3]
        );
    }
}
