//! `.bus` / `.ovh` road vehicle definitions (unit `mc_roadvehicle`) and `.zug` train lists.

use omsi_cfg::CfgFile;
use std::path::{Path, PathBuf};

mod lists;
mod offered;
mod parse;
mod plates;
#[cfg(test)]
mod tests;
mod types;

pub use self::lists::{NumberList, Train};
pub use self::offered::{front_sections_of, offered_vehicles};
pub use self::parse::parse_attachment;
pub use self::types::*;
