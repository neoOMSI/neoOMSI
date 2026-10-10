use super::*;

pub(super) fn words() -> Words {
    let t = |key: &str| ::user_interface::tr(&format!("ingame.navigator.{key}")).into_owned();
    Words {
        kmh: t("kmh"),
        days: [
            t("days.mon"),
            t("days.tue"),
            t("days.wed"),
            t("days.thu"),
            t("days.fri"),
            t("days.sat"),
            t("days.sun"),
        ],
        off_route: t("off_route"),
        rerouting: t("rerouting"),
        recalculated: t("recalculated"),
        jam: t("jam"),
        slow: t("slow"),
        map: t("map"),
        last_stop: t("last_stop"),
        on_time: t("on_time"),
    }
}