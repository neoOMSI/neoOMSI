use super::*;

/// `.zug`: pairs of lines (vehicle file, reverse flag).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Train {
    pub cars: Vec<(String, bool)>,
}

impl Train {
    pub fn load(path: &Path) -> Result<Train, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        let mut t = Train::default();
        let lines: Vec<&String> = f.lines.iter().filter(|l| !l.trim().is_empty()).collect();
        for pair in lines.chunks(2) {
            let file = pair[0].trim().to_string();
            let rev = pair.get(1).map(|s| s.trim() == "1").unwrap_or(false);
            t.cars.push((file, rev));
        }
        Ok(t)
    }
}

/// A `[number]` list (`.org`): one fleet number a line - every line, the first as well.
/// (The stock `.bus` files describe a first line naming a repaint; Omsi.exe 2.3 reads none:
/// 0x614f90 takes each non-empty line for a number.)
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NumberList {
    pub numbers: Vec<String>,
}

impl NumberList {
    pub fn load(path: &Path) -> Result<NumberList, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        let numbers = f
            .lines
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(NumberList { numbers })
    }
}
