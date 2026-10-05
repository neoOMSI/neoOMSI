use super::*;

impl Vehicle {
    /// The fleet numbers of the `[number]` list with the plate `[registration_list]`'s file
    /// gives each - line by line beside it, empty lines counted (0x614f90) - and no plate
    /// where that file has none.
    pub fn numbers_with_plates(&self) -> Vec<(String, String)> {
        let Some(list) = self.number_file.as_ref() else {
            return Vec::new();
        };
        // (read once per bus: the AI asks it for every bus it puts on the road)
        static CACHE: std::sync::OnceLock<
            std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Vec<(String, String)>>>,
        > = std::sync::OnceLock::new();
        let key = self.path.clone();
        if let Some(v) = CACHE
            .get_or_init(Default::default)
            .lock()
            .ok()
            .and_then(|c| c.get(&key).cloned())
        {
            return v;
        }
        let out = self.read_numbers_with_plates(list);
        if let Ok(mut c) = CACHE.get_or_init(Default::default).lock() {
            c.insert(key, out.clone());
        }
        out
    }

    fn read_numbers_with_plates(&self, list: &str) -> Vec<(String, String)> {
        let Ok(numbers) = CfgFile::read(&omsi_cfg::resolve_path(self.dir(), list)) else {
            return Vec::new();
        };
        let plates = self
            .registration_list
            .as_ref()
            .and_then(|(f, _, _)| CfgFile::read(&omsi_cfg::resolve_path(self.dir(), f)).ok());
        numbers
            .lines
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.trim().is_empty())
            .map(|(i, n)| {
                let plate = plates
                    .as_ref()
                    .and_then(|p| p.lines.get(i))
                    .map(|p| p.trim_end().to_string())
                    .unwrap_or_default();
                (n.trim().to_string(), plate)
            })
            .collect()
    }

    /// The plate of fleet number `number`, as Omsi.exe gives it to an AI bus not in the
    /// `[registration_free]` mode (0x7e7a80, from the depot buses' 0x70a174): the
    /// `[registration_list]` file's plate of that number when it has one, whatever the mode,
    /// else prefix, number and postfix of the list or automatic mode - the number alone
    /// without a mode.
    pub fn plate_of_number(&self, number: &str) -> String {
        self.plate_from(number, self.registration_list.is_some())
    }

    /// The plate the vehicle dialog gives the player's bus for fleet number `number`
    /// (Tform_selectVeh.Edit1Change, which Button1Click writes over the AI's): the list
    /// file's plate only in the list mode, the last plate keyword's - a repaint's
    /// `[registration_list]` followed by the template's `[registration_automatic]` gives
    /// the player prefix and number, its AI copies the list's plate.
    pub fn chosen_plate_of_number(&self, number: &str) -> String {
        self.plate_from(number, self.registration_mode == 2)
    }

    fn plate_from(&self, number: &str, list: bool) -> String {
        if list {
            if let Some((_, p)) = self
                .numbers_with_plates()
                .into_iter()
                .find(|(n, p)| n == number.trim() && !p.is_empty())
            {
                return p;
            }
        }
        let (pre, post) = (&self.registration_affix.0, &self.registration_affix.1);
        format!("{pre}{}{post}", number.trim())
    }
}
