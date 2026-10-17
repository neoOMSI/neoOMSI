use imgui::StyleColor as C;

pub(super) const ROBOTO: &[u8] =
    include_bytes!("../../../../assets/fonts/Roboto-VariableFont_wdth,wght.ttf");
pub(super) const FONT_PX: f32 = 16.0;

fn c(r: u8, g: u8, b: u8, a: f32) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

pub(super) fn apply(style: &mut imgui::Style) {
    let panel = c(22, 22, 22, 0.97);
    let panel_alt = c(31, 31, 31, 1.0);
    let sidebar = c(15, 15, 15, 1.0);
    let hot = c(38, 38, 40, 1.0);
    let selected = c(44, 44, 46, 1.0);
    let border = c(255, 255, 255, 0.06);
    let accent = c(232, 160, 48, 1.0);
    let accent_hot = c(246, 182, 84, 1.0);
    let accent_soft = c(232, 160, 48, 0.22);
    let text = c(236, 236, 236, 1.0);
    let muted = c(142, 142, 142, 1.0);
    let clear = c(0, 0, 0, 0.0);

    style.window_rounding = 8.0;
    style.child_rounding = 6.0;
    style.popup_rounding = 8.0;
    style.frame_rounding = 6.0;
    style.grab_rounding = 6.0;
    style.scrollbar_rounding = 6.0;
    style.tab_rounding = 6.0;
    style.window_border_size = 1.0;
    style.popup_border_size = 1.0;
    style.frame_border_size = 0.0;
    style.window_padding = [12.0, 12.0];
    style.frame_padding = [10.0, 5.0];
    style.item_spacing = [8.0, 6.0];
    style.item_inner_spacing = [6.0, 4.0];
    style.scrollbar_size = 12.0;
    style.grab_min_size = 12.0;
    style.window_title_align = [0.0, 0.5];

    let colors = [
        (C::Text, text),
        (C::TextDisabled, muted),
        (C::WindowBg, panel),
        (C::ChildBg, clear),
        (C::PopupBg, panel),
        (C::Border, border),
        (C::BorderShadow, clear),
        (C::FrameBg, panel_alt),
        (C::FrameBgHovered, hot),
        (C::FrameBgActive, selected),
        (C::TitleBg, sidebar),
        (C::TitleBgActive, sidebar),
        (C::TitleBgCollapsed, sidebar),
        (C::MenuBarBg, sidebar),
        (C::ScrollbarBg, clear),
        (C::ScrollbarGrab, c(60, 60, 64, 1.0)),
        (C::ScrollbarGrabHovered, c(80, 80, 86, 1.0)),
        (C::ScrollbarGrabActive, accent),
        (C::CheckMark, accent),
        (C::SliderGrab, accent),
        (C::SliderGrabActive, accent_hot),
        (C::Button, panel_alt),
        (C::ButtonHovered, hot),
        (C::ButtonActive, selected),
        (C::Header, c(255, 255, 255, 0.06)),
        (C::HeaderHovered, c(255, 255, 255, 0.09)),
        (C::HeaderActive, accent_soft),
        (C::Separator, border),
        (C::SeparatorHovered, accent),
        (C::SeparatorActive, accent_hot),
        (C::Tab, panel_alt),
        (C::TabHovered, hot),
        (C::TabActive, selected),
        (C::TextSelectedBg, accent_soft),
        (C::DragDropTarget, accent),
        (C::NavHighlight, accent),
        (C::TableHeaderBg, sidebar),
        (C::TableBorderStrong, border),
        (C::TableBorderLight, border),
    ];
    for (k, v) in colors {
        style[k] = v;
    }
}