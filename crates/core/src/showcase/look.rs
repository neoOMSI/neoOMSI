use crate::*;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Look {
    pub root: PathBuf,
    pub map: String,
    pub bus: String,
    pub paint: String,
    pub weather: String,
    pub time: i32,
    pub date: String,
}

impl Look {
    pub(super) fn same_model(&self, o: &Look) -> bool {
        self.bus == o.bus && self.paint == o.paint && self.map == o.map && self.root == o.root
    }

    pub(super) fn args(&self) -> Args {
        let mut v = vec![
            "omsi".to_string(),
            "--root".into(),
            self.root.to_string_lossy().to_string(),
        ];
        for (flag, val) in [
            ("--map", &self.map),
            ("--bus", &self.bus),
            ("--paint", &self.paint),
        ] {
            if !val.is_empty() {
                v.extend([flag.to_string(), val.clone()]);
            }
        }
        v.extend(["--time".into(), "12:00".into()]);
        Args::try_parse_from(v).unwrap_or_else(|_| Args::parse_from(["omsi"]))
    }
}
