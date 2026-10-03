//! `.odr` driver profiles (unit `mc_driver`).

use omsi_cfg::CfgFile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Driver {
    pub path: PathBuf,
    pub name: String,
    pub sex: String,
    pub birth_date: i32,
    pub employ_date: i32,
    pub bus_stops: [i32; 3],
    pub hektom: f64,
    pub crashes: [i32; 4],
    pub tickets: [f64; 2],
    pub rating: [f64; 5],
    pub per_bus_info: Vec<String>,
}

impl Driver {
    /// Write the personnel file the way OMSI does (UTF-16 LE with a BOM, CR LF).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut t = String::from(
            "-----------------------\r\nDriver File\r\n-----------------------\r\n\r\n",
        );
        t.push_str("Created with neoOMSI\r\n\r\n");
        t.push_str(&format!(
            "[ident]\r\n{}\r\n{}\r\n{}\r\n{}\r\n\r\n",
            self.name,
            if self.sex.is_empty() { "M" } else { &self.sex },
            self.birth_date,
            self.employ_date
        ));
        t.push_str(&format!(
            "[busstops]\r\n{}\r\n{}\r\n{}\r\n\r\n",
            self.bus_stops[0], self.bus_stops[1], self.bus_stops[2]
        ));
        t.push_str(&format!("[hektom]\r\n{:.0}\r\n\r\n", self.hektom));
        t.push_str(&format!(
            "[crashs]\r\n{}\r\n{}\r\n{}\r\n{}\r\n\r\n",
            self.crashes[0], self.crashes[1], self.crashes[2], self.crashes[3]
        ));
        t.push_str(&format!(
            "[tickets]\r\n{:.0}\r\n{:.6}\r\n\r\n",
            self.tickets[0], self.tickets[1]
        ));
        t.push_str("[rating]\r\n");
        for v in &self.rating {
            t.push_str(&format!("{v:.6}\r\n"));
        }
        if !self.per_bus_info.is_empty() {
            t.push_str("\r\n[perbusinfo]\r\n");
            for l in &self.per_bus_info {
                t.push_str(&format!("{l}\r\n"));
            }
        }
        let mut bytes = vec![0xFF, 0xFE];
        for u in t.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(path, bytes)
    }

    pub fn load(path: &Path) -> Result<Driver, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        let mut d = Driver {
            path: f.path.clone(),
            ..Default::default()
        };
        let mut r = f.reader();
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "ident" => {
                    d.name = r.str().to_string();
                    d.sex = r.str().to_string();
                    d.birth_date = r.i32();
                    d.employ_date = r.i32();
                }
                "busstops" => d.bus_stops = [r.i32(), r.i32(), r.i32()],
                "hektom" => d.hektom = r.f64(),
                "crashs" => d.crashes = [r.i32(), r.i32(), r.i32(), r.i32()],
                "tickets" => d.tickets = r.f64s::<2>(),
                "rating" => d.rating = r.f64s::<5>(),
                "perbusinfo" => {
                    d.per_bus_info = r
                        .rest_of_block()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect()
                }
                _ => {}
            }
        }
        d.check_ratings();
        Ok(d)
    }

    /// The `[rating]` block is OMSI's driver record from +0x40 on (the original,
    /// the original): the driving penalty P (0..1), passengers who stepped in
    /// without a complaint, tickets asked for, points for selling them (2 for the right
    /// change, 1 for the wrong), passengers who stepped in. Values that cannot be those (an
    /// older neoOMSI file kept averages there) are set back to zero.
    fn check_ratings(&mut self) {
        let [p, content, asked, points, stepped] = self.rating;
        let ok = (0.0..=1.0).contains(&p)
            && content >= 0.0
            && content <= stepped
            && asked >= 0.0
            && points >= 0.0
            && points <= 2.0 * asked + 1e-9
            && self.rating.iter().all(|v| v.is_finite());
        if !ok {
            log::info!(
                "personnel file {}: ratings {:?} are not Omsi.exe's counters; started afresh",
                self.path.display(),
                self.rating
            );
            self.rating = [0.0; 5];
        }
    }

    /// Driving as the personnel dialog shows it: 100 (1 − P) per cent.
    pub fn driving_percent(&self) -> f64 {
        100.0 * (1.0 - self.rating[0].clamp(0.0, 1.0))
    }

    /// Passenger comfort: the share who stepped in without a complaint (None before anybody).
    pub fn comfort_percent(&self) -> Option<f64> {
        (self.rating[4] > 0.0).then(|| 100.0 * self.rating[1] / self.rating[4])
    }

    /// Ticket selling: points over twice the tickets asked for (None before any).
    pub fn ticket_percent(&self) -> Option<f64> {
        (self.rating[2] > 0.0).then(|| 100.0 * self.rating[3] / (2.0 * self.rating[2]))
    }
}
