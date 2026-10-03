//! Frame-rate governor: how often mirrors are redrawn and how the render scale follows the frame rate.

/// Mirror pictures drawn per second at most, all mirrors together (see the redraw).
pub(super) const MIRROR_RATE: f32 = 75.0;
/// The least a mirror is redrawn a second (see the mirrors in `redraw_render`).
pub(super) const MIRROR_MIN_HZ: f32 = 8.0;
/// The most a mirror in the picture is redrawn a second, with the real-time reflections
/// economical (`mirror_refresh=eco`) and full (the default).
pub(super) const MIRROR_MAX_HZ_ECO: f32 = 15.0;
pub(super) const MIRROR_MAX_HZ_FULL: f32 = 30.0;
/// With no real-time reflections (`mirror_refresh=off`) a bus's mirrors are drawn once when
/// it is taken over and once more this many seconds later.
pub(super) const MIRROR_FREEZE_REDRAW: f32 = 2.0;

/// Consume the VR redraw budget without updating a mirror twice in one frame.
/// Negative rates request every mirror each frame; zero freezes immediately.
pub(super) fn vr_mirror_updates(budget: &mut f32, dt: f32, rate: f32, mirrors: usize) -> usize {
    if mirrors == 0 || rate == 0.0 {
        *budget = 0.0;
        return 0;
    }
    if rate < 0.0 {
        *budget = 0.0;
        return mirrors;
    }
    *budget = (*budget + dt.clamp(0.0, 0.1) * rate).min(mirrors as f32 + 0.5);
    let updates = (budget.floor() as usize).min(mirrors);
    *budget -= updates as f32;
    updates
}

pub(super) fn render_scale_step(fps: f32, slow_frame_wait_share: f32) -> f32 {
    // (three levels, far apart, and a wide band between going down and up again: every
    // step makes the picture's targets anew - hundreds of MB with MSAA and HDR - and a
    // scale that went up and down by 5 % every two seconds stuttered at each change and
    // filled the card's memory with the old ones until the driver gave up)
    if fps < 40.0 && slow_frame_wait_share >= 0.4 {
        -0.15
    } else if fps > 58.0 || slow_frame_wait_share < 0.2 {
        0.15
    } else {
        0.0
    }
}

#[cfg(test)]
mod governor_tests {
    use super::render_scale_step;

    #[test]
    fn cpu_stutters_do_not_reduce_picture_quality() {
        assert!(render_scale_step(35.0, 0.1) > 0.0);
        assert!(render_scale_step(35.0, 0.6) < 0.0);
        assert!(render_scale_step(60.0, 0.6) > 0.0);
    }
}

#[cfg(test)]
mod vr_mirror_tests {
    use super::vr_mirror_updates;

    #[test]
    fn every_frame_updates_all_mirrors_even_at_low_game_fps() {
        let mut budget = 0.75;
        for dt in [1.0 / 90.0, 1.0 / 30.0, 0.5] {
            assert_eq!(vr_mirror_updates(&mut budget, dt, -1.0, 8), 8);
            assert_eq!(budget, 0.0);
        }
    }

    #[test]
    fn a_high_budget_is_not_limited_to_two_mirrors_per_frame() {
        let mut budget = 0.0;
        assert_eq!(vr_mirror_updates(&mut budget, 1.0 / 60.0, 240.0, 4), 4);
        assert_eq!(vr_mirror_updates(&mut budget, 0.5, 360.0, 4), 4);
        assert!(budget <= 0.5);
    }

    #[test]
    fn fractional_credit_preserves_the_selected_total_rate() {
        for fps in [30, 60, 90] {
            let mut budget = 0.0;
            let updates: usize = (0..fps * 10)
                .map(|_| vr_mirror_updates(&mut budget, 1.0 / fps as f32, 16.0, 4))
                .sum();
            assert!((159..=160).contains(&updates), "fps={fps}: {updates}");
        }
    }

    #[test]
    fn off_and_no_mirrors_discard_old_credit() {
        let mut budget = 2.5;
        assert_eq!(vr_mirror_updates(&mut budget, 0.1, 0.0, 4), 0);
        assert_eq!(budget, 0.0);
        assert_eq!(vr_mirror_updates(&mut budget, 0.1, -1.0, 0), 0);
        assert_eq!(vr_mirror_updates(&mut budget, 0.1, 360.0, 0), 0);
        assert_eq!(budget, 0.0);
    }
}
