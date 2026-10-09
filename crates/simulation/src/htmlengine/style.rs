//! Computed style: lengths, colours and the property table a page can set.

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Len {
    Px(f32),
    Pct(f32),
}

impl Len {
    pub(crate) fn px(self, base: f32) -> f32 {
        match self {
            Len::Px(v) => v,
            Len::Pct(p) => base * p / 100.0,
        }
    }
}

/// `background-size`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BgSize {
    Auto,
    Cover,
    Contain,
    /// Width and height; `None` is `auto` (keeps the picture's proportions).
    Dims(Option<Len>, Option<Len>),
}

#[derive(Debug, Clone)]
pub(crate) struct Style {
    pub(crate) color: [u8; 4],
    pub(crate) bg: [u8; 4],
    pub(crate) font_px: f32,
    pub(crate) bold: bool,
    pub(crate) align: u8,
    pub(crate) line_h: f32,
    pub(crate) hidden: bool,
    pub(crate) margin: [f32; 4],
    pub(crate) margin_auto: bool,
    pub(crate) padding: [f32; 4],
    pub(crate) width: Option<Len>,
    pub(crate) height: Option<Len>,
    pub(crate) none: bool,
    pub(crate) inline: bool,
    /// `display:inline-block`: a box that sits in a row with its neighbours.
    pub(crate) inline_block: bool,
    pub(crate) radius: f32,
    /// `background-image: url(...)`: the path as written.
    pub(crate) bg_img: Option<Arc<str>>,
    pub(crate) bg_size: BgSize,
    /// `background-repeat`: (horizontally, vertically).
    pub(crate) bg_repeat: (bool, bool),
    /// `background-position`: (x, y); a percentage places the picture's own point at the
    /// same point of the box.
    pub(crate) bg_pos: [Len; 2],
    /// Height / width of an `<img>` whose width is a percentage (0: not used).
    pub(crate) aspect: f32,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) pre: bool,
    pub(crate) nowrap: bool,
    pub(crate) transform: u8,
    pub(crate) rise: f32,
    pub(crate) valign: u8,
    pub(crate) border: [f32; 4],
    pub(crate) border_color: Option<[u8; 4]>,
    pub(crate) spacing: f32,
    pub(crate) collapse: bool,
    pub(crate) list: u8,
    /// The element this style was computed for (what a text run belongs to when it is hit).
    pub(crate) node: usize,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            color: [0, 0, 0, 255],
            bg: [0, 0, 0, 0],
            font_px: 16.0,
            bold: false,
            align: 0,
            line_h: 1.2,
            hidden: false,
            margin: [0.0; 4],
            margin_auto: false,
            padding: [0.0; 4],
            width: None,
            height: None,
            none: false,
            inline: false,
            inline_block: false,
            radius: 0.0,
            bg_img: None,
            bg_size: BgSize::Auto,
            bg_repeat: (true, true),
            bg_pos: [Len::Pct(0.0), Len::Pct(0.0)],
            aspect: 0.0,
            underline: false,
            strike: false,
            pre: false,
            nowrap: false,
            transform: 0,
            rise: 0.0,
            valign: 0,
            border: [0.0; 4],
            border_color: None,
            spacing: 0.0,
            collapse: false,
            list: 0,
            node: 0,
        }
    }
}

impl Style {
    pub(crate) fn inherit(&self) -> Style {
        Style {
            color: self.color,
            font_px: self.font_px,
            bold: self.bold,
            align: self.align,
            line_h: self.line_h,
            hidden: self.hidden,
            underline: self.underline,
            strike: self.strike,
            pre: self.pre,
            nowrap: self.nowrap,
            transform: self.transform,
            rise: self.rise,
            list: self.list,
            ..Style::default()
        }
    }
}

pub(crate) fn parse_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(h) = s.strip_prefix('#') {
        let d: Vec<u8> = h
            .chars()
            .map(|c| c.to_digit(16).map(|v| v as u8))
            .collect::<Option<Vec<_>>>()?;
        return match d.len() {
            3 => Some([d[0] * 17, d[1] * 17, d[2] * 17, 255]),
            4 => Some([d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17]),
            6 => Some([d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], 255]),
            8 => Some([
                d[0] * 16 + d[1],
                d[2] * 16 + d[3],
                d[4] * 16 + d[5],
                d[6] * 16 + d[7],
            ]),
            _ => None,
        };
    }
    if let Some(a) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")) {
        let inner = a.trim_end_matches(')');
        let p: Vec<f32> = inner
            .split(|c| c == ',' || c == ' ' || c == '/')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| match x.strip_suffix('%') {
                Some(pc) => pc.parse::<f32>().unwrap_or(0.0) * 2.55,
                None => x.parse::<f32>().unwrap_or(0.0),
            })
            .collect();
        if p.len() < 3 {
            return None;
        }
        let alpha = p
            .get(3)
            .map_or(255.0, |a| if *a <= 1.0 { a * 255.0 } else { *a });
        return Some([
            p[0].clamp(0.0, 255.0) as u8,
            p[1].clamp(0.0, 255.0) as u8,
            p[2].clamp(0.0, 255.0) as u8,
            alpha.clamp(0.0, 255.0) as u8,
        ]);
    }
    let named = match s.as_str() {
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "lime" => [0, 255, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "orange" => [255, 165, 0, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "silver" => [192, 192, 192, 255],
        "cyan" | "aqua" => [0, 255, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255, 255],
        "transparent" => [0, 0, 0, 0],
        _ => return None,
    };
    Some(named)
}

pub(crate) struct Units {
    pub(crate) font: f32,
    pub(crate) vw: f32,
    pub(crate) vh: f32,
}

pub(crate) fn parse_len(v: &str, u: &Units) -> Option<Len> {
    let v = v.trim().to_ascii_lowercase();
    let num = |suffix: &str| {
        v.strip_suffix(suffix)
            .and_then(|n| n.trim().parse::<f32>().ok())
    };
    if v == "0" {
        return Some(Len::Px(0.0));
    }
    if let Some(n) = num("px") {
        return Some(Len::Px(n));
    }
    if let Some(n) = num("%") {
        return Some(Len::Pct(n));
    }
    if let Some(n) = num("rem") {
        return Some(Len::Px(n * 16.0));
    }
    if let Some(n) = num("em") {
        return Some(Len::Px(n * u.font));
    }
    if let Some(n) = num("pt") {
        return Some(Len::Px(n * 4.0 / 3.0));
    }
    if let Some(n) = num("vw") {
        return Some(Len::Px(n * u.vw / 100.0));
    }
    if let Some(n) = num("vh") {
        return Some(Len::Px(n * u.vh / 100.0));
    }
    v.parse::<f32>().ok().map(Len::Px)
}

/// Split a value at spaces and at `/`, but not inside parentheses (`rgba(0, 0, 0, .5)`,
/// `url(a b.png)`); a `/` is a token of its own.
pub(crate) fn split_top(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        match c {
            '(' => {
                depth += 1;
                start.get_or_insert(i);
            }
            ')' => depth = (depth - 1).max(0),
            c if depth == 0 && (c.is_whitespace() || c == '/') => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
                if c == '/' {
                    out.push("/");
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// The first `url(...)` of a value: the path (quotes removed) and the byte range of the
/// whole `url(...)` in `val`.
pub(crate) fn find_url(val: &str) -> Option<(String, usize, usize)> {
    let s = val.to_ascii_lowercase().find("url(")?;
    let inner = s + 4;
    let e = val[inner..].find(')')? + inner;
    let path = val[inner..e]
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_string();
    Some((path, s, e + 1))
}

pub(crate) fn parse_bg_size(t: &[&str], u: &Units) -> Option<BgSize> {
    let len = |x: &str| {
        if x == "auto" {
            Some(None)
        } else {
            parse_len(x, u).map(Some)
        }
    };
    match t {
        ["cover"] => Some(BgSize::Cover),
        ["contain"] => Some(BgSize::Contain),
        [a] => Some(BgSize::Dims(len(*a)?, None)),
        [a, b] => Some(BgSize::Dims(len(*a)?, len(*b)?)),
        _ => None,
    }
}

pub(crate) fn parse_bg_pos(t: &[&str], u: &Units) -> Option<[Len; 2]> {
    let val = |x: &str| match x {
        "left" | "top" => Some(Len::Pct(0.0)),
        "center" => Some(Len::Pct(50.0)),
        "right" | "bottom" => Some(Len::Pct(100.0)),
        _ => parse_len(x, u),
    };
    match t {
        [a] => {
            let v = val(*a)?;
            Some(if matches!(*a, "top" | "bottom") {
                [Len::Pct(50.0), v]
            } else {
                [v, Len::Pct(50.0)]
            })
        }
        [a, b] => {
            let (x, y) = (val(*a)?, val(*b)?);
            Some(
                if matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right") {
                    [y, x]
                } else {
                    [x, y]
                },
            )
        }
        _ => None,
    }
}

pub(crate) fn list_kind(t: &str) -> Option<u8> {
    Some(match t {
        "none" => 0,
        "disc" => 1,
        "circle" => 2,
        "square" => 3,
        "decimal" | "decimal-leading-zero" => 4,
        "lower-alpha" | "lower-latin" => 5,
        "upper-alpha" | "upper-latin" => 6,
        "lower-roman" => 7,
        "upper-roman" => 8,
        _ => return None,
    })
}

pub(crate) fn parse_border(val: &str, u: &Units) -> (f32, Option<[u8; 4]>, bool) {
    let (mut w, mut c, mut none, mut styled) = (None, None, false, false);
    for t in split_top(val) {
        match t.to_ascii_lowercase().as_str() {
            "none" | "hidden" => none = true,
            "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "inset" | "outset" => {
                styled = true
            }
            "thin" => w = Some(1.0),
            "medium" => w = Some(3.0),
            "thick" => w = Some(5.0),
            _ => {
                if let Some(col) = parse_color(t) {
                    c = Some(col);
                } else if let Some(l) = parse_len(t, u) {
                    w = Some(l.px(u.vw));
                }
            }
        }
    }
    (w.unwrap_or(if styled { 3.0 } else { 0.0 }), c, none)
}

pub(crate) fn box_values(v: &str, u: &Units) -> ([f32; 4], bool) {
    let toks: Vec<&str> = v.split_whitespace().collect();
    let mut auto = false;
    let vals: Vec<f32> = toks
        .iter()
        .map(|t| {
            if *t == "auto" {
                auto = true;
                0.0
            } else {
                parse_len(t, u).map_or(0.0, |l| l.px(u.vw))
            }
        })
        .collect();
    let out = match vals.len() {
        1 => [vals[0]; 4],
        2 => [vals[0], vals[1], vals[0], vals[1]],
        3 => [vals[0], vals[1], vals[2], vals[1]],
        4 => [vals[0], vals[1], vals[2], vals[3]],
        _ => [0.0; 4],
    };
    (out, auto)
}

impl Style {
    pub(crate) fn apply(&mut self, prop: &str, val: &str, parent_font: f32, vw: f32, vh: f32) {
        let val = val.trim().trim_end_matches("!important").trim();
        let u = Units {
            font: parent_font,
            vw,
            vh,
        };
        let own = Units {
            font: self.font_px,
            vw,
            vh,
        };
        match prop {
            "color" => {
                if let Some(c) = parse_color(val) {
                    self.color = c;
                }
            }
            "background-color" => {
                if let Some(c) = split_top(val).into_iter().find_map(parse_color) {
                    self.bg = c;
                }
            }
            "background" => {
                // the shorthand resets what it does not name (colour excepted, as before)
                self.bg_img = None;
                self.bg_size = BgSize::Auto;
                self.bg_repeat = (true, true);
                self.bg_pos = [Len::Pct(0.0), Len::Pct(0.0)];
                let mut rest = val.to_string();
                if let Some((path, s, e)) = find_url(val) {
                    if !path.is_empty() {
                        self.bg_img = Some(Arc::from(path.as_str()));
                    }
                    rest.replace_range(s..e, " ");
                }
                let (mut pos, mut size, mut slash) = (Vec::new(), Vec::new(), false);
                for t in split_top(&rest) {
                    if t == "/" {
                        slash = true;
                    } else if let Some(c) = parse_color(t) {
                        self.bg = c;
                    } else {
                        match t {
                            "no-repeat" => self.bg_repeat = (false, false),
                            "repeat" => self.bg_repeat = (true, true),
                            "repeat-x" => self.bg_repeat = (true, false),
                            "repeat-y" => self.bg_repeat = (false, true),
                            "none" => self.bg_img = None,
                            _ if slash => size.push(t),
                            _ => pos.push(t),
                        }
                    }
                }
                if let Some(p) = parse_bg_pos(&pos, &own) {
                    self.bg_pos = p;
                }
                if let Some(z) = parse_bg_size(&size, &own) {
                    self.bg_size = z;
                }
            }
            "background-image" => {
                self.bg_img = find_url(val).and_then(|(p, _, _)| {
                    if p.is_empty() {
                        None
                    } else {
                        Some(Arc::from(p.as_str()))
                    }
                });
            }
            "background-size" => {
                if let Some(z) = parse_bg_size(&split_top(val), &own) {
                    self.bg_size = z;
                }
            }
            "background-repeat" => match val {
                "no-repeat" => self.bg_repeat = (false, false),
                "repeat" => self.bg_repeat = (true, true),
                "repeat-x" => self.bg_repeat = (true, false),
                "repeat-y" => self.bg_repeat = (false, true),
                _ => {}
            },
            "background-position" => {
                if let Some(p) = parse_bg_pos(&split_top(val), &own) {
                    self.bg_pos = p;
                }
            }
            "font-size" => {
                let named = match val {
                    "small" => Some(13.0),
                    "medium" => Some(16.0),
                    "large" => Some(18.0),
                    "x-large" => Some(24.0),
                    "xx-large" => Some(32.0),
                    _ => None,
                };
                if let Some(px) = named {
                    self.font_px = px;
                } else if let Some(l) = parse_len(val, &u) {
                    self.font_px = l.px(parent_font).max(1.0);
                }
            }
            "font-weight" => {
                self.bold = matches!(val, "bold" | "bolder")
                    || val.parse::<u32>().map_or(false, |w| w >= 600);
            }
            "text-align" => {
                self.align = match val {
                    "center" => 1,
                    "right" | "end" => 2,
                    _ => 0,
                };
            }
            "line-height" => {
                if let Ok(f) = val.parse::<f32>() {
                    self.line_h = f;
                } else if let Some(l) = parse_len(val, &own) {
                    self.line_h = l.px(self.font_px) / self.font_px;
                }
            }
            "margin" => {
                let (v, a) = box_values(val, &own);
                self.margin = v;
                self.margin_auto = a;
            }
            "margin-top" => self.margin[0] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "margin-right" => {
                self.margin_auto |= val == "auto";
                self.margin[1] = parse_len(val, &own).map_or(0.0, |l| l.px(vw));
            }
            "margin-bottom" => self.margin[2] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "margin-left" => {
                self.margin_auto |= val == "auto";
                self.margin[3] = parse_len(val, &own).map_or(0.0, |l| l.px(vw));
            }
            "padding" => self.padding = box_values(val, &own).0,
            "padding-top" => self.padding[0] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "padding-right" => self.padding[1] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "padding-bottom" => self.padding[2] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "padding-left" => self.padding[3] = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            "width" => {
                self.width = if val == "auto" {
                    None
                } else {
                    parse_len(val, &own)
                }
            }
            "height" => {
                self.height = if val == "auto" {
                    None
                } else {
                    parse_len(val, &own)
                }
            }
            "display" => {
                self.none = val == "none";
                self.inline = val == "inline";
                self.inline_block = val == "inline-block";
            }
            "text-decoration" | "text-decoration-line" => {
                let v = val.to_ascii_lowercase();
                if v.contains("none") {
                    self.underline = false;
                    self.strike = false;
                } else {
                    self.underline |= v.contains("underline");
                    self.strike |= v.contains("line-through");
                }
            }
            "white-space" => {
                let (p, n) = match val {
                    "nowrap" => (false, true),
                    "pre" => (true, true),
                    "pre-wrap" | "pre-line" | "break-spaces" => (true, false),
                    _ => (false, false),
                };
                self.pre = p;
                self.nowrap = n;
            }
            "text-transform" => {
                self.transform = match val {
                    "uppercase" => 1,
                    "lowercase" => 2,
                    "capitalize" => 3,
                    _ => 0,
                }
            }
            "vertical-align" => match val {
                "super" => self.rise = 0.4,
                "sub" => self.rise = -0.2,
                "top" | "text-top" => {
                    self.valign = 0;
                    self.rise = 0.0;
                }
                "middle" => self.valign = 1,
                "bottom" | "text-bottom" => self.valign = 2,
                _ => self.rise = 0.0,
            },
            "list-style-type" | "list-style" => {
                for t in split_top(val) {
                    if let Some(l) = list_kind(t) {
                        self.list = l;
                    }
                }
            }
            "border" => {
                let (w, c, none) = parse_border(val, &own);
                self.border = [if none { 0.0 } else { w }; 4];
                if c.is_some() {
                    self.border_color = c;
                }
            }
            "border-top" | "border-right" | "border-bottom" | "border-left" => {
                let i = match prop {
                    "border-top" => 0,
                    "border-right" => 1,
                    "border-bottom" => 2,
                    _ => 3,
                };
                let (w, c, none) = parse_border(val, &own);
                self.border[i] = if none { 0.0 } else { w };
                if c.is_some() {
                    self.border_color = c;
                }
            }
            "border-width" => self.border = box_values(val, &own).0,
            "border-top-width" | "border-right-width" | "border-bottom-width"
            | "border-left-width" => {
                let i = match prop {
                    "border-top-width" => 0,
                    "border-right-width" => 1,
                    "border-bottom-width" => 2,
                    _ => 3,
                };
                self.border[i] = match val {
                    "thin" => 1.0,
                    "medium" => 3.0,
                    "thick" => 5.0,
                    _ => parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
                };
            }
            "border-color" | "border-top-color" | "border-right-color" | "border-bottom-color"
            | "border-left-color" => {
                if let Some(c) = split_top(val).into_iter().find_map(parse_color) {
                    self.border_color = Some(c);
                }
            }
            "border-style" => {
                if val.contains("none") || val.contains("hidden") {
                    self.border = [0.0; 4];
                }
            }
            "border-top-style" | "border-right-style" | "border-bottom-style"
            | "border-left-style" => {
                if val == "none" || val == "hidden" {
                    let i = match prop {
                        "border-top-style" => 0,
                        "border-right-style" => 1,
                        "border-bottom-style" => 2,
                        _ => 3,
                    };
                    self.border[i] = 0.0;
                }
            }
            "border-collapse" => {
                self.collapse = val == "collapse";
                if self.collapse {
                    self.spacing = 0.0;
                }
            }
            "border-spacing" => {
                self.spacing = parse_len(val.split_whitespace().next().unwrap_or(""), &own)
                    .map_or(0.0, |l| l.px(vw));
            }
            "visibility" => self.hidden = val == "hidden",
            "border-radius" => self.radius = parse_len(val, &own).map_or(0.0, |l| l.px(vw)),
            _ => {}
        }
    }
}
