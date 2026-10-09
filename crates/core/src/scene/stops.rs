use super::*;

impl World {
    /// How many passengers get off at stop object `id` (see `tiles::stop_exit_weight`; a
    /// stop without strings: the defaults' mean, 0.5).
    pub fn stop_exit_weight(&self, id: i64) -> f32 {
        self.index().stop_weights.get(&id).copied().unwrap_or(0.5)
    }

    /// Stop object `id`'s (pass_enter_max, pass_enter_min) (see `tiles::stop_enter`; a
    /// stop without strings: the defaults, 1 and 0).
    pub fn stop_enter(&self, id: i64) -> (f32, f32) {
        self.index()
            .stop_enter
            .get(&id)
            .copied()
            .unwrap_or((1.0, 0.0))
    }

    /// The side stop object `id`'s platform lies on (see `tiles::stop_side`): 0 = right,
    /// 1 = the other, 2 = both; a stop the map says nothing about: 0.
    pub fn stop_side(&self, id: i64) -> f32 {
        self.index().stop_side.get(&id).copied().unwrap_or(0.0)
    }

    /// Stop object `id`'s length (see `tiles::stop_length`; 30 m when the map says nothing).
    pub fn stop_length(&self, id: i64) -> f32 {
        self.index().stop_length.get(&id).copied().unwrap_or(30.0)
    }
}
