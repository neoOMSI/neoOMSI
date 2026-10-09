//! Game settings menu

use super::*;

pub(super) const PAGE: Page = Page {
    nav: "pause.page.options.nav",
    draw: Ui::draw_options_page,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fmt {
    /// 0.75 -> "75 %"
    Pct,
    /// 0.05 -> "+5 cm"
    Cm,
    /// 1.5 -> "x1.5"
    Times,
    /// 30 -> "30°"
    Deg,
    /// below 20: "Default", else degrees
    Fov,
    /// below 45: "OMSI", else degrees
    Lock,
    /// 0.25 -> "250 ms"
    Ms,
    /// 6 -> "6/15"
    Of15,
    /// 1.3 -> "1.30"
    Dec2,
}

impl Fmt {
    pub fn apply(self, v: f32) -> String {
        match self {
            Fmt::Pct => format!("{:.0} %", v * 100.0),
            Fmt::Cm => format!("{:+.0} cm", v * 100.0),
            Fmt::Times => format!("x{v}"),
            Fmt::Deg => format!("{v:.0}°"),
            Fmt::Fov if v < 20.0 => t("pause.options.fmt.default"),
            Fmt::Fov => format!("{v:.0}°"),
            Fmt::Lock if v < 45.0 => "OMSI".to_string(),
            Fmt::Lock => format!("{v:.0}°"),
            Fmt::Ms => format!("{:.0} ms", v * 1000.0),
            Fmt::Of15 => format!("{}/15", v as i64),
            Fmt::Dec2 => format!("{v:.2}"),
        }
    }
}

/// What a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptKind {
    Switch,
    Slider(Fmt),
    Select,
    Preset,
    Opens,
    Button(&'static str),
    Keybinds,
    Pads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptShow {
    Always,
    Windows,
    VrOn,
    VrBus,
    Profiles,
}

#[derive(Clone, Copy, Debug)]
pub struct OptRow {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub kind: OptKind,
    pub show: OptShow,
}

#[derive(Clone, Copy, Debug)]
pub struct OptSub {
    pub title: &'static str,
    pub rows: &'static [OptRow],
}

#[derive(Clone, Copy, Debug)]
pub struct OptGroup {
    pub title: &'static str,
    pub tab: &'static str,
    pub rows: &'static [OptRow],
    pub subs: &'static [OptSub],
}

const fn row(id: &'static str, name: &'static str, desc: &'static str, kind: OptKind) -> OptRow {
    OptRow {
        id,
        name,
        desc,
        kind,
        show: OptShow::Always,
    }
}
const fn sw(id: &'static str, name: &'static str, desc: &'static str) -> OptRow {
    row(id, name, desc, OptKind::Switch)
}
const fn sl(id: &'static str, name: &'static str, desc: &'static str, f: Fmt) -> OptRow {
    row(id, name, desc, OptKind::Slider(f))
}
const fn sel(id: &'static str, name: &'static str, desc: &'static str) -> OptRow {
    row(id, name, desc, OptKind::Select)
}
const fn when(r: OptRow, show: OptShow) -> OptRow {
    OptRow { show, ..r }
}

pub const OPTION_GROUPS: &[OptGroup] = &[
    OptGroup {
        title: "pause.options.group.gameplay",
        tab: "",
        rows: &[
            sw(
                "auto_ibis",
                "pause.options.gameplay.auto_ibis.name",
                "pause.options.gameplay.auto_ibis.desc",
            ),
            sw(
                "exact_fare",
                "pause.options.gameplay.exact_fare.name",
                "pause.options.gameplay.exact_fare.desc",
            ),
            sel(
                "pax_motion",
                "pause.options.gameplay.pax_motion.name",
                "pause.options.gameplay.pax_motion.desc",
            ),
            sw(
                "pax_ik",
                "pause.options.gameplay.pax_ik.name",
                "pause.options.gameplay.pax_ik.desc",
            ),
            sel(
                "pax_models",
                "pause.options.gameplay.pax_models.name",
                "pause.options.gameplay.pax_models.desc",
            ),
            sel(
                "boarding",
                "pause.options.gameplay.boarding.name",
                "pause.options.gameplay.boarding.desc",
            ),
            sw(
                "pax_prefer_seats",
                "pause.options.gameplay.pax_prefer_seats.name",
                "pause.options.gameplay.pax_prefer_seats.desc",
            ),
            sw(
                "pax_rear_entry",
                "pause.page.text.boarding_at_the_rear_doors",
                "pause.page.text.passengers_who_need_no_ticket_from_the_driver_also_get_on_at_the_rear_doors",
            ),
            sel(
                "maintenance",
                "pause.options.gameplay.maintenance.name",
                "pause.options.later",
            ),
            sw(
                "coll_objects",
                "pause.options.gameplay.coll_objects.name",
                "pause.options.gameplay.coll_objects.desc",
            ),
            sw(
                "coll_vehicles",
                "pause.options.gameplay.coll_vehicles.name",
                "pause.options.gameplay.coll_vehicles.desc",
            ),
            sw(
                "collision_pedestrians",
                "pause.options.gameplay.collision_pedestrians.name",
                "pause.options.gameplay.collision_pedestrians.desc",
            ),
            sel(
                "ai_unsched_factor",
                "pause.options.gameplay.ai_unsched_factor.name",
                "pause.options.later",
            ),
            sel(
                "ai_max_scheduled",
                "pause.options.gameplay.ai_max_scheduled.name",
                "pause.options.later",
            ),
            sel(
                "ai_max_parked",
                "pause.options.gameplay.ai_max_parked.name",
                "pause.options.later",
            ),
        ],
        subs: &[],
    },
    OptGroup {
        title: "pause.options.group.driving",
        tab: "",
        rows: &[
            sw(
                "auto_clutch",
                "pause.options.driving.auto_clutch.name",
                "pause.options.driving.auto_clutch.desc",
            ),
            sw(
                "auto_shift",
                "pause.options.driving.auto_shift.name",
                "pause.options.driving.auto_shift.desc",
            ),
            sw(
                "momentary_gears",
                "pause.options.driving.momentary_gears.name",
                "pause.options.later",
            ),
            sw(
                "brake_hold",
                "pause.options.driving.brake_hold.name",
                "pause.options.driving.brake_hold.desc",
            ),
            sw(
                "blinker_cancel",
                "pause.options.driving.blinker_cancel.name",
                "pause.options.driving.blinker_cancel.desc",
            ),
            sw(
                "steering_linear",
                "pause.options.driving.steering_linear.name",
                "pause.options.driving.steering_linear.desc",
            ),
            sw(
                "old_steering",
                "pause.options.driving.old_steering.name",
                "pause.options.driving.old_steering.desc",
            ),
            sw(
                "red_steer_spd",
                "pause.options.driving.red_steer_spd.name",
                "pause.options.driving.red_steer_spd.desc",
            ),
        ],
        subs: &[],
    },
    OptGroup {
        title: "pause.options.group.controls",
        tab: "pause.options.group.general",
        rows: &[
            sw(
                "mouse",
                "pause.options.controls.mouse.name",
                "pause.options.controls.mouse.desc",
            ),
            sw(
                "mouse_right",
                "pause.options.controls.mouse_right.name",
                "pause.options.controls.mouse_right.desc",
            ),
            sl(
                "mouse_sens",
                "pause.options.controls.mouse_sens.name",
                "pause.options.controls.mouse_sens.desc",
                Fmt::Pct,
            ),
            sl(
                "stick_sens",
                "pause.options.controls.stick_sens.name",
                "pause.options.controls.stick_sens.desc",
                Fmt::Pct,
            ),
            sw(
                "steer_center",
                "pause.options.controls.steer_center.name",
                "pause.options.controls.steer_center.desc",
            ),
            sl(
                "pedal_t",
                "pause.options.controls.pedal_t.name",
                "pause.options.controls.pedal_t.desc",
                Fmt::Times,
            ),
            sl(
                "pedal_b",
                "pause.options.controls.pedal_b.name",
                "pause.options.controls.pedal_b.desc",
                Fmt::Times,
            ),
            sl(
                "wheel_range",
                "pause.options.controls.wheel_range.name",
                "pause.options.controls.wheel_range.desc",
                Fmt::Deg,
            ),
            sl(
                "wheel_lock",
                "pause.options.controls.wheel_lock.name",
                "pause.options.controls.wheel_lock.desc",
                Fmt::Lock,
            ),
            sw(
                "ff",
                "pause.options.controls.ff.name",
                "pause.options.controls.ff.desc",
            ),
            sw(
                "ff_invert",
                "pause.options.controls.ff_invert.name",
                "pause.options.controls.ff_invert.desc",
            ),
            row(
                "pads",
                "pause.options.controls.pads.name",
                "pause.options.controls.pads.desc",
                OptKind::Pads,
            ),
        ],
        subs: &[OptSub {
            title: "pause.options.group.keybinds",
            rows: &[row(
                "keybinds",
                "pause.options.controls.keybinds.name",
                "pause.options.controls.keybinds.desc",
                OptKind::Keybinds,
            )],
        }],
    },
    OptGroup {
        title: "pause.options.group.camera",
        tab: "",
        rows: &[
            sl(
                "fov",
                "pause.options.camera.fov.name",
                "pause.options.camera.fov.desc",
                Fmt::Fov,
            ),
            sw(
                "head",
                "pause.options.camera.head.name",
                "pause.options.camera.head.desc",
            ),
            sw(
                "cam_smooth",
                "pause.options.camera.cam_smooth.name",
                "pause.options.camera.cam_smooth.desc",
            ),
            sw(
                "camcoll",
                "pause.options.camera.camcoll.name",
                "pause.options.camera.camcoll.desc",
            ),
            sl(
                "look_sens",
                "pause.options.camera.look_sens.name",
                "pause.options.camera.look_sens.desc",
                Fmt::Pct,
            ),
            sw(
                "alt_view",
                "pause.options.camera.alt_view.name",
                "pause.options.camera.alt_view.desc",
            ),
            sw(
                "steer_look",
                "pause.options.camera.steer_look.name",
                "pause.options.camera.steer_look.desc",
            ),
            sl(
                "steer_look_angle",
                "pause.options.camera.steer_look_angle.name",
                "pause.options.camera.steer_look_angle.desc",
                Fmt::Deg,
            ),
            sl(
                "steer_look_response",
                "pause.options.camera.steer_look_response.name",
                "pause.options.camera.steer_look_response.desc",
                Fmt::Ms,
            ),
            sw(
                "hands_in_cab",
                "pause.options.camera.hands_in_cab.name",
                "pause.options.camera.hands_in_cab.desc",
            ),
            sw(
                "driver",
                "pause.options.camera.driver.name",
                "pause.options.camera.driver.desc",
            ),
            sl(
                "seat 1",
                "pause.options.camera.seat_1.name",
                "pause.options.camera.seat_1.desc",
                Fmt::Cm,
            ),
            sl(
                "seat 2",
                "pause.options.camera.seat_2.name",
                "pause.options.camera.seat_2.desc",
                Fmt::Cm,
            ),
            sl(
                "seat 0",
                "pause.options.camera.seat_0.name",
                "pause.options.camera.seat_0.desc",
                Fmt::Cm,
            ),
            row(
                "seat_reset",
                "pause.options.camera.seat_reset.name",
                "pause.options.camera.seat_reset.desc",
                OptKind::Button("pause.options.button.reset"),
            ),
        ],
        subs: &[
            OptSub {
                title: "pause.options.group.head_tracking",
                rows: &[
                    sw(
                        "headtrack",
                        "pause.options.camera.headtrack.name",
                        "pause.options.camera.headtrack.desc",
                    ),
                    sel(
                        "head_tracking_port",
                        "pause.options.camera.head_tracking_port.name",
                        "pause.options.camera.head_tracking_port.desc",
                    ),
                    sel(
                        "head_tracking_invert",
                        "pause.options.camera.head_tracking_invert.name",
                        "pause.options.camera.head_tracking_invert.desc",
                    ),
                ],
            },
            OptSub {
                title: "pause.options.group.free_look",
                rows: &[
                    sw(
                        "free_look",
                        "pause.options.free_look.free_look.name",
                        "pause.options.free_look.free_look.desc",
                    ),
                    sw(
                        "crosshair",
                        "pause.options.free_look.crosshair.name",
                        "pause.options.free_look.crosshair.desc",
                    ),
                    sw(
                        "tooltips",
                        "pause.options.free_look.tooltips.name",
                        "pause.options.free_look.tooltips.desc",
                    ),
                ],
            },
        ],
    },
    OptGroup {
        title: "pause.options.group.graphics",
        tab: "",
        rows: &[
            row(
                "preset",
                "pause.options.graphics.preset.name",
                "pause.options.graphics.preset.desc",
                OptKind::Preset,
            ),
            when(
                sel(
                    "graphics_api",
                    "pause.options.graphics.graphics_api.name",
                    "pause.options.graphics.graphics_api.desc",
                ),
                OptShow::Windows,
            ),
            when(
                row(
                    "gfxprofile",
                    "pause.options.graphics.gfxprofile.name",
                    "pause.options.graphics.gfxprofile.desc",
                    OptKind::Opens,
                ),
                OptShow::Profiles,
            ),
            sel(
                "graphics",
                "pause.options.graphics.graphics.name",
                "pause.options.later",
            ),
            sel(
                "msaa",
                "pause.options.graphics.msaa.name",
                "pause.options.later",
            ),
            sel(
                "post_aa",
                "pause.options.graphics.post_aa.name",
                "pause.options.graphics.post_aa.desc",
            ),
            sel(
                "render_scale",
                "pause.options.graphics.render_scale.name",
                "pause.options.later",
            ),
            sel(
                "anisotropy",
                "pause.options.graphics.anisotropy.name",
                "pause.options.later",
            ),
            sw(
                "shadows",
                "pause.options.graphics.shadows.name",
                "pause.options.graphics.shadows.desc",
            ),
            sw(
                "shadow_blobs",
                "pause.options.graphics.shadow_blobs.name",
                "pause.options.graphics.shadow_blobs.desc",
            ),
            sel(
                "shadow_size",
                "pause.options.graphics.shadow_size.name",
                "pause.options.later",
            ),
            sel(
                "shadow_casters",
                "pause.options.graphics.shadow_casters.name",
                "pause.options.later",
            ),
            sw(
                "ssao",
                "pause.options.graphics.ssao.name",
                "pause.options.later",
            ),
            sw(
                "reflections",
                "pause.options.graphics.reflections.name",
                "pause.options.later",
            ),
            sw(
                "clouds",
                "pause.options.graphics.clouds.name",
                "pause.options.later",
            ),
            sw(
                "detail_textures",
                "pause.options.graphics.detail_textures.name",
                "pause.options.graphics.detail_textures.desc",
            ),
            sel(
                "map_detail",
                "pause.options.graphics.map_detail.name",
                "pause.options.later",
            ),
            sel(
                "view_distance",
                "pause.options.graphics.view_distance.name",
                "pause.options.later",
            ),
            sel(
                "max_obj_dist",
                "pause.options.graphics.max_obj_dist.name",
                "pause.options.later",
            ),
            sel(
                "min_obj_size",
                "pause.options.graphics.min_obj_size.name",
                "pause.options.later",
            ),
            sel(
                "mirror_size",
                "pause.options.graphics.mirror_size.name",
                "pause.options.later",
            ),
            sel(
                "mirror_refresh",
                "pause.options.graphics.mirror_refresh.name",
                "pause.options.graphics.mirror_refresh.desc",
            ),
            sel(
                "texture_memory",
                "pause.options.graphics.texture_memory.name",
                "pause.options.later",
            ),
            sw(
                "texture_compression",
                "pause.options.graphics.texture_compression.name",
                "pause.options.later",
            ),
            sl(
                "led_glow",
                "pause.options.graphics.led_glow.name",
                "pause.options.graphics.led_glow.desc",
                Fmt::Of15,
            ),
            sl(
                "nightmap_glow",
                "pause.options.graphics.nightmap_glow.name",
                "pause.options.graphics.nightmap_glow.desc",
                Fmt::Of15,
            ),
            sl(
                "atmosphere_brightness",
                "pause.options.graphics.atmosphere_brightness.name",
                "pause.options.graphics.atmosphere_brightness.desc",
                Fmt::Dec2,
            ),
            sl(
                "led_mips",
                "pause.options.graphics.led_mips.name",
                "pause.options.graphics.led_mips.desc",
                Fmt::Dec2,
            ),
        ],
        subs: &[],
    },
    OptGroup {
        title: "pause.options.group.display",
        tab: "",
        rows: &[
            sw(
                "fullscreen",
                "pause.options.display.fullscreen.name",
                "pause.options.display.fullscreen.desc",
            ),
            sw(
                "vsync",
                "pause.options.display.vsync.name",
                "pause.options.display.vsync.desc",
            ),
            sel(
                "max_fps",
                "pause.options.display.max_fps.name",
                "pause.options.display.max_fps.desc",
            ),
            sw(
                "fps",
                "pause.options.display.fps.name",
                "pause.options.display.fps.desc",
            ),
        ],
        subs: &[],
    },
    OptGroup {
        title: "pause.options.group.sound",
        tab: "",
        rows: &[
            sl(
                "volume",
                "pause.options.sound.volume.name",
                "pause.options.sound.volume.desc",
                Fmt::Pct,
            ),
            sl(
                "vol_ai",
                "pause.options.sound.vol_ai.name",
                "pause.options.sound.vol_ai.desc",
                Fmt::Pct,
            ),
            sl(
                "vol_scenery",
                "pause.options.sound.vol_scenery.name",
                "pause.options.sound.vol_scenery.desc",
                Fmt::Pct,
            ),
            sw(
                "doppler",
                "pause.options.sound.doppler.name",
                "pause.options.sound.doppler.desc",
            ),
            sel(
                "pax_voices",
                "pause.options.sound.pax_voices.name",
                "pause.options.sound.pax_voices.desc",
            ),
        ],
        subs: &[],
    },
    OptGroup {
        title: "pause.options.group.interface",
        tab: "",
        rows: &[
            sel(
                "language",
                "pause.options.interface.language.name",
                "pause.options.interface.language.desc",
            ),
            sel(
                "units",
                "pause.options.interface.units.name",
                "pause.options.interface.units.desc",
            ),
            sl(
                "ui_scale",
                "pause.options.interface.ui_scale.name",
                "pause.options.interface.ui_scale.desc",
                Fmt::Pct,
            ),
            sw(
                "ui_scale_window",
                "pause.options.interface.ui_scale_window.name",
                "pause.options.interface.ui_scale_window.desc",
            ),
            sl(
                "ui_opacity",
                "pause.options.interface.ui_opacity.name",
                "pause.options.interface.ui_opacity.desc",
                Fmt::Pct,
            ),
            sw(
                "nav_arrows",
                "pause.options.interface.nav_arrows.name",
                "pause.options.interface.nav_arrows.desc",
            ),
            sw(
                "info_bar",
                "pause.options.interface.info_bar.name",
                "pause.options.interface.info_bar.desc",
            ),
            sw(
                "timetable_win",
                "pause.options.interface.timetable_win.name",
                "pause.options.interface.timetable_win.desc",
            ),
            sw(
                "notes",
                "pause.options.interface.notes.name",
                "pause.options.interface.notes.desc",
            ),
            sw(
                "tooltips",
                "pause.options.interface.tooltips.name",
                "pause.options.interface.tooltips.desc",
            ),
            sw(
                "chat",
                "pause.options.interface.chat.name",
                "pause.options.interface.chat.desc",
            ),
            sw(
                "discord_status",
                "pause.options.interface.discord_status.name",
                "pause.options.interface.discord_status.desc",
            ),
            sw(
                "name_tags",
                "pause.options.interface.name_tags.name",
                "pause.options.interface.name_tags.desc",
            ),
            row(
                "reset",
                "pause.options.interface.reset.name",
                "pause.options.interface.reset.desc",
                OptKind::Opens,
            ),
        ],
        subs: &[OptSub {
            title: "pause.options.group.map",
            rows: &[
                sw(
                    "navigator",
                    "pause.options.map.navigator.name",
                    "pause.options.map.navigator.desc",
                ),
                sw(
                    "nav_topbar",
                    "pause.options.map.nav_topbar.name",
                    "pause.options.map.nav_topbar.desc",
                ),
                sw(
                    "nav_turn",
                    "pause.options.map.nav_turn.name",
                    "pause.options.map.nav_turn.desc",
                ),
                sw(
                    "nav_stoplist",
                    "pause.options.map.nav_stoplist.name",
                    "pause.options.map.nav_stoplist.desc",
                ),
                sw(
                    "nav_stops_ext",
                    "pause.options.map.nav_stops_ext.name",
                    "pause.options.map.nav_stops_ext.desc",
                ),
                sw(
                    "nav_ai",
                    "pause.options.map.nav_ai.name",
                    "pause.options.map.nav_ai.desc",
                ),
                sel(
                    "navigator_corner",
                    "pause.options.map.navigator_corner.name",
                    "pause.options.later",
                ),
            ],
        }],
    },
    OptGroup {
        title: "pause.options.group.vr",
        tab: "",
        rows: &[
            when(
                sw("vr", "pause.options.vr.vr.name", "pause.options.later"),
                OptShow::Windows,
            ),
            when(
                sel(
                    "vr_scale",
                    "pause.options.vr.vr_scale.name",
                    "pause.options.later",
                ),
                OptShow::VrOn,
            ),
            when(
                sel(
                    "vr_head_smoothing_ms",
                    "pause.options.vr.vr_head_smoothing_ms.name",
                    "pause.options.later",
                ),
                OptShow::VrOn,
            ),
            when(
                sel(
                    "vr_mirror_rate",
                    "pause.options.vr.vr_mirror_rate.name",
                    "pause.options.later",
                ),
                OptShow::VrOn,
            ),
            when(
                sw(
                    "vr_desktop_mirror",
                    "pause.options.vr.vr_desktop_mirror.name",
                    "pause.options.later",
                ),
                OptShow::VrOn,
            ),
            when(
                sw(
                    "navigator",
                    "pause.options.vr.navigator.name",
                    "pause.options.vr.navigator.desc",
                ),
                OptShow::VrBus,
            ),
            when(
                row(
                    "vr_nav_edit",
                    "pause.options.vr.vr_nav_edit.name",
                    "pause.options.vr.vr_nav_edit.desc",
                    OptKind::Button("pause.options.button.open"),
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_x",
                    "pause.options.vr.vr_nav_x.name",
                    "pause.options.vr.vr_nav_x.desc",
                    Fmt::Cm,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_y",
                    "pause.options.vr.vr_nav_y.name",
                    "pause.options.vr.vr_nav_y.desc",
                    Fmt::Cm,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_z",
                    "pause.options.vr.vr_nav_z.name",
                    "pause.options.vr.vr_nav_z.desc",
                    Fmt::Cm,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_width",
                    "pause.options.vr.vr_nav_width.name",
                    "pause.options.vr.vr_nav_width.desc",
                    Fmt::Cm,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_yaw",
                    "pause.options.vr.vr_nav_yaw.name",
                    "pause.options.vr.vr_nav_yaw.desc",
                    Fmt::Deg,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_tilt",
                    "pause.options.vr.vr_nav_tilt.name",
                    "pause.options.vr.vr_nav_tilt.desc",
                    Fmt::Deg,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_roll",
                    "pause.options.vr.vr_nav_roll.name",
                    "pause.options.vr.vr_nav_roll.desc",
                    Fmt::Deg,
                ),
                OptShow::VrBus,
            ),
            when(
                sl(
                    "vr_nav_opacity",
                    "pause.options.vr.vr_nav_opacity.name",
                    "pause.options.vr.vr_nav_opacity.desc",
                    Fmt::Pct,
                ),
                OptShow::VrBus,
            ),
            when(
                row(
                    "vr_nav_reset",
                    "pause.options.vr.vr_nav_reset.name",
                    "pause.options.vr.vr_nav_reset.desc",
                    OptKind::Button("pause.options.button.reset"),
                ),
                OptShow::VrBus,
            ),
        ],
        subs: &[],
    },
];

impl Ui {
    pub(super) fn draw_options_page(
        &mut self,
        r: &Renderer,
        scene: &mut Scene,
        f: &Frame,
        m: Metrics,
        top: f32,
        pt: f32,
    ) {
        self.draw_group_page(
            r,
            scene,
            f,
            m,
            top,
            pt,
            OPTIONS_PAGE,
            "pause.page.options.head",
            "pause.page.options.note",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_pct_cm_times() {
        assert_eq!(Fmt::Pct.apply(0.75), "75 %");
        assert_eq!(Fmt::Pct.apply(0.0), "0 %");
        assert_eq!(Fmt::Cm.apply(0.05), "+5 cm");
        assert_eq!(Fmt::Cm.apply(-0.1), "-10 cm");
        assert_eq!(Fmt::Times.apply(1.5), "x1.5");
        assert_eq!(Fmt::Times.apply(2.0), "x2");
    }

    #[test]
    fn fmt_angles() {
        assert_eq!(Fmt::Deg.apply(30.0), "30°");
        assert_eq!(Fmt::Fov.apply(90.0), "90°");
        assert_eq!(Fmt::Fov.apply(20.0), "20°");
        assert_eq!(Fmt::Lock.apply(10.0), "OMSI");
        assert_eq!(Fmt::Lock.apply(44.9), "OMSI");
        assert_eq!(Fmt::Lock.apply(45.0), "45°");
    }

    #[test]
    fn fmt_fov_below_20_is_default_label() {
        assert_eq!(Fmt::Fov.apply(0.0), t("pause.options.fmt.default"));
        assert_eq!(Fmt::Fov.apply(19.9), t("pause.options.fmt.default"));
    }

    #[test]
    fn fmt_misc() {
        assert_eq!(Fmt::Ms.apply(0.25), "250 ms");
        assert_eq!(Fmt::Of15.apply(6.0), "6/15");
        assert_eq!(Fmt::Of15.apply(6.9), "6/15");
        assert_eq!(Fmt::Dec2.apply(1.3), "1.30");
    }

    #[test]
    fn row_helpers_set_kind_and_default_show() {
        let r = sw("a", "n", "d");
        assert_eq!((r.id, r.name, r.desc), ("a", "n", "d"));
        assert_eq!(r.kind, OptKind::Switch);
        assert_eq!(r.show, OptShow::Always);
        assert_eq!(sl("a", "n", "d", Fmt::Pct).kind, OptKind::Slider(Fmt::Pct));
        assert_eq!(sel("a", "n", "d").kind, OptKind::Select);
    }

    #[test]
    fn when_only_changes_visibility() {
        let r = when(sw("a", "n", "d"), OptShow::VrOn);
        assert_eq!(r.show, OptShow::VrOn);
        assert_eq!(r.kind, OptKind::Switch);
        assert_eq!(r.id, "a");
    }

    #[test]
    fn option_groups_are_well_formed() {
        assert!(!OPTION_GROUPS.is_empty());
        for g in OPTION_GROUPS {
            assert!(g.title.starts_with("pause."), "{}", g.title);
            assert!(!g.rows.is_empty() || !g.subs.is_empty(), "{}", g.title);
            let subs = g.subs.iter().flat_map(|s| s.rows.iter());
            for r in g.rows.iter().chain(subs) {
                assert!(!r.id.is_empty());
                assert!(!r.name.is_empty());
                assert!(!r.desc.is_empty());
            }
            for s in g.subs {
                assert!(!s.title.is_empty());
                assert!(!s.rows.is_empty(), "{}", s.title);
            }
        }
    }

    #[test]
    fn ids_unique_within_a_group() {
        for g in OPTION_GROUPS {
            let mut ids: Vec<&str> = g.rows.iter().map(|r| r.id).collect();
            ids.extend(g.subs.iter().flat_map(|s| s.rows.iter().map(|r| r.id)));
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "{}", g.title);
        }
    }

    #[test]
    fn button_rows_have_a_label() {
        for g in OPTION_GROUPS {
            for r in g
                .rows
                .iter()
                .chain(g.subs.iter().flat_map(|s| s.rows.iter()))
            {
                if let OptKind::Button(label) = r.kind {
                    assert!(!label.is_empty(), "{}", r.id);
                }
            }
        }
    }
}
