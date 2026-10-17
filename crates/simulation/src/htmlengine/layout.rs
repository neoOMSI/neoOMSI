//! Block flow and wrapped inline text: turns the styled page into boxes, and finds the
//! box under a point.

use super::*;

pub(crate) struct LItem {
    pub(crate) text: String,
    pub(crate) px: f32,
    pub(crate) bold: bool,
    pub(crate) color: [u8; 4],
    pub(crate) dx: f32,
    pub(crate) w: f32,
    pub(crate) node: usize,
    pub(crate) bg: [u8; 4],
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) rise: f32,
}

pub(crate) struct LLine {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) h: f32,
    pub(crate) items: Vec<LItem>,
}

pub(crate) enum Item {
    Block(LBox),
    Line(LLine),
}

pub(crate) struct LBox {
    pub(crate) rect: [f32; 4],
    pub(crate) mb: f32,
    pub(crate) st: Style,
    pub(crate) items: Vec<Item>,
    pub(crate) node: usize,
    pub(crate) marker: Option<String>,
}

pub(crate) fn push_text(runs: &mut Vec<(String, Style)>, t: &str, st: &Style) {
    if st.pre && t.contains('\n') {
        for (i, part) in t.split('\n').enumerate() {
            if i > 0 {
                runs.push(("\n".into(), st.clone()));
            }
            if !part.is_empty() {
                runs.push((part.to_string(), st.clone()));
            }
        }
    } else {
        runs.push((t.to_string(), st.clone()));
    }
}

fn transform_text(t: &str, kind: u8) -> String {
    match kind {
        1 => t.to_uppercase(),
        2 => t.to_lowercase(),
        3 => {
            let mut out = String::with_capacity(t.len());
            let mut start = true;
            for c in t.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
                start = c.is_whitespace();
            }
            out
        }
        _ => t.to_string(),
    }
}

fn alpha_num(mut n: i64, upper: bool) -> String {
    if n < 1 {
        return n.to_string();
    }
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, (b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    if upper { s.to_ascii_uppercase() } else { s }
}

fn roman_num(mut n: i64, upper: bool) -> String {
    if !(1..4000).contains(&n) {
        return n.to_string();
    }
    let t = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper { s.to_ascii_uppercase() } else { s }
}

fn attr_color(s: &str) -> Option<[u8; 4]> {
    parse_color(s).or_else(|| parse_color(&format!("#{}", s.trim())))
}

/// `<font size>`: 1..7 or relative to 3.
fn font_size_attr(s: &str, base: f32) -> f32 {
    let s = s.trim();
    let n = match (s.strip_prefix('+'), s.strip_prefix('-')) {
        (Some(d), _) => 3 + d.parse::<i32>().unwrap_or(0),
        (_, Some(d)) => 3 - d.parse::<i32>().unwrap_or(0),
        _ => match s.parse::<i32>() {
            Ok(n) => n,
            Err(_) => return base,
        },
    };
    [10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0][(n.clamp(1, 7) - 1) as usize]
}

/// Move a laid-out box (and everything in it).
pub(crate) fn shift(b: &mut LBox, dx: f32, dy: f32) {
    b.rect[0] += dx;
    b.rect[1] += dy;
    for it in &mut b.items {
        match it {
            Item::Block(c) => shift(c, dx, dy),
            Item::Line(l) => {
                l.x += dx;
                l.y += dy;
            }
        }
    }
}

/// How wide the content of a box wants to be, counted from `cx` (its content's left edge).
pub(crate) fn natural_width(b: &LBox, cx: f32) -> f32 {
    let mut w = 0.0f32;
    for it in &b.items {
        match it {
            Item::Line(l) => {
                if let Some(li) = l.items.last() {
                    w = w.max(li.dx + li.w);
                }
            }
            Item::Block(c) => w = w.max(c.rect[0] - cx + c.rect[2] + c.st.margin[1]),
        }
    }
    w
}

/// The element under a point: the text run or box on top, `None` outside every box.
pub(crate) fn hit(b: &LBox, x: f32, y: f32) -> Option<usize> {
    if b.st.hidden {
        return None;
    }
    for it in b.items.iter().rev() {
        match it {
            Item::Block(c) => {
                if let Some(n) = hit(c, x, y) {
                    return Some(n);
                }
            }
            Item::Line(l) => {
                if y >= l.y && y < l.y + l.h {
                    for li in &l.items {
                        let x0 = l.x + li.dx;
                        if x >= x0 && x < x0 + li.w {
                            return Some(li.node);
                        }
                    }
                }
            }
        }
    }
    let [rx, ry, rw, rh] = b.rect;
    (x >= rx && x < rx + rw && y >= ry && y < ry + rh).then_some(b.node)
}

pub(crate) struct Layouter<'a> {
    pub(crate) dom: &'a Dom,
    pub(crate) reg: &'a FontRef<'static>,
    pub(crate) bold: &'a FontRef<'static>,
    pub(crate) vw: f32,
    pub(crate) vh: f32,
    pub(crate) imgs: &'a ImageStore,
}

impl<'a> Layouter<'a> {
    pub(crate) fn tw(&self, s: &str, px: f32, bold: bool) -> f32 {
        let font = if bold { self.bold } else { self.reg };
        let sf = font.as_scaled(PxScale::from(px));
        let mut w = 0.0;
        let mut prev = None;
        for c in s.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                w += sf.kern(p, id);
            }
            w += sf.h_advance(id);
            prev = Some(id);
        }
        w
    }

    pub(crate) fn style_of(&self, idx: usize, ps: &Style) -> Style {
        let n = &self.dom.nodes[idx];
        let mut st = ps.inherit();
        let pf = ps.font_px;
        let tag = n.tag.as_str();
        let ctl_border = Some([0x76, 0x76, 0x76, 255]);
        match tag {
            "b" | "strong" => {
                st.inline = true;
                st.bold = true;
            }
            "span" | "i" | "em" | "cite" | "dfn" | "var" | "label" | "code" | "kbd" | "samp"
            | "tt" | "abbr" | "acronym" | "time" | "data" | "bdi" | "bdo" | "nobr" | "ruby"
            | "rt" | "output" | "q" | "wbr" => st.inline = true,
            "a" => {
                st.inline = true;
                if self.dom.attr(idx, "href").is_some() {
                    st.color = [0, 0, 238, 255];
                    st.underline = true;
                }
            }
            "small" => {
                st.inline = true;
                st.font_px = pf * 0.83;
            }
            "big" => {
                st.inline = true;
                st.font_px = pf * 1.2;
            }
            "u" | "ins" => {
                st.inline = true;
                st.underline = true;
            }
            "s" | "strike" | "del" => {
                st.inline = true;
                st.strike = true;
            }
            "mark" => {
                st.inline = true;
                st.bg = [255, 255, 0, 255];
                st.color = [0, 0, 0, 255];
            }
            "sup" => {
                st.inline = true;
                st.font_px = pf * 0.75;
                st.rise = 0.4;
            }
            "sub" => {
                st.inline = true;
                st.font_px = pf * 0.75;
                st.rise = -0.2;
            }
            "font" => {
                st.inline = true;
                if let Some(c) = self.dom.attr(idx, "color").and_then(attr_color) {
                    st.color = c;
                }
                if let Some(sz) = self.dom.attr(idx, "size") {
                    st.font_px = font_size_attr(sz, pf);
                }
            }
            "h1" => {
                st.font_px = pf * 2.0;
                st.bold = true;
                st.margin[0] = st.font_px * 0.67;
                st.margin[2] = st.font_px * 0.67;
            }
            "h2" => {
                st.font_px = pf * 1.5;
                st.bold = true;
                st.margin[0] = st.font_px * 0.83;
                st.margin[2] = st.font_px * 0.83;
            }
            "h3" => {
                st.font_px = pf * 1.17;
                st.bold = true;
                st.margin[0] = st.font_px;
                st.margin[2] = st.font_px;
            }
            "h4" => {
                st.font_px = pf;
                st.bold = true;
                st.margin[0] = st.font_px * 1.33;
                st.margin[2] = st.font_px * 1.33;
            }
            "h5" => {
                st.font_px = pf * 0.83;
                st.bold = true;
                st.margin[0] = st.font_px * 1.67;
                st.margin[2] = st.font_px * 1.67;
            }
            "h6" => {
                st.font_px = pf * 0.67;
                st.bold = true;
                st.margin[0] = st.font_px * 2.33;
                st.margin[2] = st.font_px * 2.33;
            }
            "p" | "dl" => {
                st.margin[0] = pf;
                st.margin[2] = pf;
            }
            "blockquote" | "figure" => st.margin = [pf, 40.0, pf, 40.0],
            "center" | "caption" => st.align = 1,
            "pre" => {
                st.margin[0] = pf;
                st.margin[2] = pf;
                st.pre = true;
                st.nowrap = true;
            }
            "ul" | "ol" | "menu" | "dir" => {
                let depth = self.count_ancestors(idx, &["ul", "ol", "menu", "dir"]);
                if self.count_ancestors(idx, &["ul", "ol", "menu", "dir", "li"]) == 0 {
                    st.margin[0] = pf;
                    st.margin[2] = pf;
                }
                st.padding[3] = 40.0;
                st.list = if tag == "ol" {
                    match self.dom.attr(idx, "type") {
                        Some("a") => 5,
                        Some("A") => 6,
                        Some("i") => 7,
                        Some("I") => 8,
                        _ => 4,
                    }
                } else {
                    match depth {
                        0 => 1,
                        1 => 2,
                        _ => 3,
                    }
                };
            }
            "dd" => st.margin[3] = 40.0,
            "hr" => {
                st.margin = [8.0, 0.0, 8.0, 0.0];
                st.border = [1.0; 4];
                st.border_color = Some([0x9a, 0x9a, 0x9a, 255]);
            }
            "fieldset" => {
                st.margin = [0.0, 2.0, 0.0, 2.0];
                st.padding = [6.0, 12.0, 8.0, 12.0];
                st.border = [1.0; 4];
                st.border_color = Some([0xc0, 0xc0, 0xc0, 255]);
            }
            "table" => {
                st.spacing = 2.0;
                let num = |k: &str| {
                    self.dom
                        .attr(idx, k)
                        .and_then(|v| v.trim().parse::<f32>().ok())
                };
                if let Some(b) = num("border").filter(|b| *b > 0.0) {
                    st.border = [b; 4];
                    st.border_color = Some([0x80, 0x80, 0x80, 255]);
                }
                if let Some(c) = num("cellspacing") {
                    st.spacing = c;
                }
            }
            "td" | "th" => {
                let tbl = self.dom.closest(idx, "table");
                let tnum = |k: &str| {
                    tbl.and_then(|t| self.dom.attr(t, k))
                        .and_then(|v| v.trim().parse::<f32>().ok())
                };
                st.padding = [tnum("cellpadding").unwrap_or(1.0); 4];
                st.valign = 1;
                if tnum("border").map_or(false, |b| b > 0.0) {
                    st.border = [1.0; 4];
                    st.border_color = Some([0x80, 0x80, 0x80, 255]);
                }
                if tag == "th" {
                    st.bold = true;
                    st.align = 1;
                }
            }
            "body" => st.margin = [8.0; 4],
            "button" => {
                st.inline_block = true;
                st.bg = [0xe0, 0xe0, 0xe0, 255];
                st.color = [0, 0, 0, 255];
                st.padding = [6.0, 12.0, 6.0, 12.0];
                st.align = 1;
                st.radius = 4.0;
            }
            "input" => self.input_style(idx, &mut st),
            "textarea" => {
                let num = |k: &str, d: f32| {
                    self.dom
                        .attr(idx, k)
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .unwrap_or(d)
                };
                st.inline_block = true;
                st.pre = true;
                st.nowrap = false;
                st.bg = [255; 4];
                st.color = [0, 0, 0, 255];
                st.padding = [2.0; 4];
                st.border = [1.0; 4];
                st.border_color = ctl_border;
                st.width = Some(Len::Px(num("cols", 20.0) * 7.5));
                st.height = Some(Len::Px(num("rows", 2.0) * st.font_px * st.line_h));
            }
            "select" => {
                st.inline_block = true;
                st.bg = [255; 4];
                st.color = [0, 0, 0, 255];
                st.padding = [2.0, 22.0, 2.0, 6.0];
                st.border = [1.0; 4];
                st.border_color = ctl_border;
                st.radius = 3.0;
            }
            "progress" | "meter" => {
                st.inline_block = true;
                st.width = Some(Len::Px(if tag == "meter" { 80.0 } else { 160.0 }));
                st.height = Some(Len::Px(16.0));
                st.bg = [0xe6, 0xe6, 0xe6, 255];
                st.radius = 8.0;
            }
            "img" | "canvas" | "video" | "svg" | "iframe" | "embed" | "object" => {
                st.inline_block = true
            }
            "dialog" => st.none = self.dom.attr(idx, "open").is_none(),
            "head" | "style" | "script" | "title" | "meta" | "link" | "template" | "noscript"
            | "datalist" | "option" | "optgroup" | "map" | "area" | "base" | "param" | "track"
            | "source" | "audio" | "rp" => st.none = true,
            _ => {}
        }
        // presentational attributes (CSS below wins over them)
        if self.dom.attr(idx, "hidden").is_some() {
            st.none = true;
        }
        if let Some(c) = self.dom.attr(idx, "bgcolor").and_then(attr_color) {
            st.bg = c;
        }
        if tag == "body" {
            if let Some(c) = self.dom.attr(idx, "text").and_then(attr_color) {
                st.color = c;
            }
        }
        if let Some(a) = self.dom.attr(idx, "align") {
            match (tag, a.to_ascii_lowercase().as_str()) {
                ("table" | "hr", "center") => st.margin_auto = true,
                ("table" | "hr" | "img" | "canvas" | "video" | "svg" | "iframe", _) => {}
                (_, "center") => st.align = 1,
                (_, "right") => st.align = 2,
                (_, "left") => st.align = 0,
                _ => {}
            }
        }
        if let Some(v) = self.dom.attr(idx, "valign") {
            match v.to_ascii_lowercase().as_str() {
                "top" => st.valign = 0,
                "middle" => st.valign = 1,
                "bottom" => st.valign = 2,
                _ => {}
            }
        }
        if self.dom.attr(idx, "nowrap").is_some() {
            st.nowrap = true;
        }
        if matches!(tag, "table" | "td" | "th" | "hr") {
            let u = Units {
                font: st.font_px,
                vw: self.vw,
                vh: self.vh,
            };
            if !n.attr_w.is_empty() {
                st.width = parse_len(&n.attr_w, &u);
            }
            if !n.attr_h.is_empty() {
                st.height = parse_len(&n.attr_h, &u);
            }
        }
        let mut matched: Vec<(u32, usize, &Rule)> = self
            .dom
            .rules
            .iter()
            .filter_map(|r| {
                r.sels
                    .iter()
                    .filter(|c| self.dom.matches(idx, c))
                    .map(specificity)
                    .max()
                    .map(|s| (s, r.order, r))
            })
            .collect();
        matched.sort_by_key(|(s, o, _)| (*s, *o));
        for (_, _, r) in matched {
            for (k, v) in &r.decls {
                st.apply(k, v, pf, self.vw, self.vh);
            }
        }
        for (k, v) in &n.inline {
            st.apply(k, v, pf, self.vw, self.vh);
        }
        if n.tag == "img" {
            self.size_img(idx, &mut st);
        } else if matches!(tag, "canvas" | "video" | "svg" | "iframe" | "embed" | "object") {
            // a replaced element without a picture: an empty box of its size (300x150 by default)
            let u = Units {
                font: st.font_px,
                vw: self.vw,
                vh: self.vh,
            };
            if st.width.is_none() {
                st.width = parse_len(&n.attr_w, &u).or(Some(Len::Px(300.0)));
            }
            if st.height.is_none() {
                st.height = parse_len(&n.attr_h, &u).or(Some(Len::Px(150.0)));
            }
        }
        // the border takes room like padding does; `paint` draws it at the outer edge
        for i in 0..4 {
            st.padding[i] += st.border[i];
        }
        st.node = idx;
        st
    }

    fn count_ancestors(&self, mut idx: usize, tags: &[&str]) -> usize {
        let mut n = 0;
        while let Some(p) = self.dom.nodes[idx].parent {
            if tags.contains(&self.dom.nodes[p].tag.as_str()) {
                n += 1;
            }
            idx = p;
        }
        n
    }

    fn input_style(&self, idx: usize, st: &mut Style) {
        let ty = self
            .dom
            .attr(idx, "type")
            .unwrap_or("text")
            .to_ascii_lowercase();
        let ctl = Some([0x76, 0x76, 0x76, 255]);
        st.inline_block = true;
        match ty.as_str() {
            "hidden" => st.none = true,
            "checkbox" | "radio" => {
                st.width = Some(Len::Px(13.0));
                st.height = Some(Len::Px(13.0));
                st.margin = [3.0, 3.0, 3.0, 4.0];
                st.bg = [255; 4];
                st.border = [1.0; 4];
                st.border_color = ctl;
                st.radius = if ty == "radio" { 7.5 } else { 2.0 };
            }
            "button" | "submit" | "reset" | "image" => {
                st.bg = [0xe0, 0xe0, 0xe0, 255];
                st.color = [0, 0, 0, 255];
                st.padding = [6.0, 12.0, 6.0, 12.0];
                st.align = 1;
                st.radius = 4.0;
                st.border = [1.0; 4];
                st.border_color = ctl;
            }
            _ => {
                let size = self
                    .dom
                    .attr(idx, "size")
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .unwrap_or(20.0);
                st.bg = [255; 4];
                st.color = [0, 0, 0, 255];
                st.padding = [2.0, 4.0, 2.0, 4.0];
                st.border = [1.0; 4];
                st.border_color = ctl;
                st.width = Some(Len::Px(size * 7.5));
                st.height = Some(Len::Px(st.font_px * st.line_h));
            }
        }
    }

    /// The text a form control shows by itself (`value`, `placeholder`, the chosen `<option>`).
    fn control_text(&self, idx: usize) -> Option<(String, Option<[u8; 4]>)> {
        let n = &self.dom.nodes[idx];
        match n.tag.as_str() {
            "input" => {
                let ty = self
                    .dom
                    .attr(idx, "type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                let val = self.dom.attr(idx, "value").unwrap_or("");
                let ph = self.dom.attr(idx, "placeholder").unwrap_or("");
                let gray = Some([0x75, 0x75, 0x75, 255]);
                match ty.as_str() {
                    "hidden" | "checkbox" | "radio" => None,
                    "button" | "submit" | "reset" | "image" => {
                        let t = if !val.is_empty() {
                            val
                        } else if ty == "submit" {
                            "Submit"
                        } else if ty == "reset" {
                            "Reset"
                        } else {
                            ""
                        };
                        (!t.is_empty()).then(|| (t.to_string(), None))
                    }
                    "password" if !val.is_empty() => {
                        Some(("\u{2022}".repeat(val.chars().count()), None))
                    }
                    _ if !val.is_empty() => Some((val.to_string(), None)),
                    _ => (!ph.is_empty()).then(|| (ph.to_string(), gray)),
                }
            }
            "select" => {
                let mut opts = Vec::new();
                for &k in &n.kids {
                    match self.dom.nodes[k].tag.as_str() {
                        "option" => opts.push(k),
                        "optgroup" => {
                            for &g in &self.dom.nodes[k].kids {
                                if self.dom.nodes[g].tag == "option" {
                                    opts.push(g);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                let pick = opts
                    .iter()
                    .copied()
                    .find(|&o| self.dom.attr(o, "selected").is_some())
                    .or(opts.first().copied())?;
                let t = collapse_ws(self.dom.text_of(pick).trim());
                (!t.is_empty()).then(|| (t, None))
            }
            _ => None,
        }
    }

    /// The text of the marker of an `<li>` (`"3."`); empty for the bullet shapes.
    fn marker_text(&self, idx: usize, kind: u8) -> String {
        if kind < 4 {
            return String::new();
        }
        let Some(p) = self.dom.nodes[idx].parent else {
            return "1.".into();
        };
        let mut counter: i64 = self
            .dom
            .attr(p, "start")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1);
        for &k in &self.dom.nodes[p].kids {
            if self.dom.nodes[k].tag != "li" {
                continue;
            }
            if let Some(v) = self
                .dom
                .attr(k, "value")
                .and_then(|v| v.trim().parse::<i64>().ok())
            {
                counter = v;
            }
            if k == idx {
                break;
            }
            counter += 1;
        }
        let t = match kind {
            5 => alpha_num(counter, false),
            6 => alpha_num(counter, true),
            7 => roman_num(counter, false),
            8 => roman_num(counter, true),
            _ => counter.to_string(),
        };
        format!("{t}.")
    }

    /// The size of an `<img>`: CSS first, then its `width`/`height` attributes, then the
    /// picture's own size; a missing side keeps the picture's proportions.
    fn size_img(&self, idx: usize, st: &mut Style) {
        let n = &self.dom.nodes[idx];
        let u = Units {
            font: st.font_px,
            vw: self.vw,
            vh: self.vh,
        };
        if st.width.is_none() && !n.attr_w.is_empty() {
            st.width = parse_len(&n.attr_w, &u);
        }
        if st.height.is_none() && !n.attr_h.is_empty() {
            st.height = parse_len(&n.attr_h, &u);
        }
        let (nw, nh) = match self.imgs.dims(&n.src) {
            Some((w, h)) if !n.src.is_empty() => (w as f32, h as f32),
            _ => (0.0, 0.0),
        };
        match (st.width, st.height) {
            (None, None) => {
                st.width = Some(Len::Px(nw));
                st.height = Some(Len::Px(nh));
            }
            (Some(Len::Px(w)), None) => {
                st.height = Some(Len::Px(if nw > 0.0 { w * nh / nw } else { 0.0 }))
            }
            (None, Some(Len::Px(h))) => {
                st.width = Some(Len::Px(if nh > 0.0 { h * nw / nh } else { 0.0 }))
            }
            (Some(Len::Pct(_)), None) if nw > 0.0 => st.aspect = nh / nw,
            _ => {}
        }
    }

    pub(crate) fn inline_runs(&self, idx: usize, st: &Style, out: &mut Vec<(String, Style)>) {
        for &k in &self.dom.nodes[idx].kids {
            let kn = &self.dom.nodes[k];
            if let Some(t) = &kn.text {
                push_text(out, t, st);
                continue;
            }
            let cs = self.style_of(k, st);
            if cs.none {
                continue;
            }
            if kn.tag == "br" {
                out.push(("\n".into(), cs));
            } else if cs.inline {
                let quote = kn.tag == "q";
                if quote {
                    out.push(("\u{201c}".into(), cs.clone()));
                }
                self.inline_runs(k, &cs, out);
                if quote {
                    out.push(("\u{201d}".into(), cs));
                }
            }
        }
    }

    pub(crate) fn finish_line(
        &self,
        items: &mut Vec<LItem>,
        lw: &mut f32,
        lh: &mut f32,
        out: &mut Vec<LLine>,
        cy: &mut f32,
        x: f32,
        w: f32,
        align: u8,
    ) {
        if let Some(l) = items.last_mut() {
            let t = l.text.trim_end().to_string();
            let nw = self.tw(&t, l.px, l.bold);
            *lw -= l.w - nw;
            l.w = nw;
            l.text = t;
        }
        let off = match align {
            1 => (w - *lw) / 2.0,
            2 => w - *lw,
            _ => 0.0,
        }
            .max(0.0);
        let h = *lh;
        out.push(LLine {
            x: x + off,
            y: *cy,
            h,
            items: std::mem::take(items),
        });
        *cy += h;
        *lw = 0.0;
        *lh = 0.0;
    }

    pub(crate) fn lines(
        &self,
        runs: &[(String, Style)],
        x: f32,
        y: f32,
        w: f32,
        align: u8,
    ) -> (Vec<LLine>, f32) {
        let mut out = Vec::new();
        let mut cy = y;
        let mut items: Vec<LItem> = Vec::new();
        let (mut lw, mut lh) = (0.0f32, 0.0f32);
        for (text, st) in runs {
            if text == "\n" {
                if lh == 0.0 {
                    lh = st.font_px * st.line_h;
                }
                self.finish_line(&mut items, &mut lw, &mut lh, &mut out, &mut cy, x, w, align);
                continue;
            }
            let tf;
            let text: &str = if st.transform != 0 {
                tf = transform_text(text, st.transform);
                &tf
            } else {
                text
            };
            for tok in text.split_inclusive(' ') {
                let full = self.tw(tok, st.font_px, st.bold);
                let t = tok.trim_end();
                let trimmed = if t.len() == tok.len() {
                    full
                } else {
                    self.tw(t, st.font_px, st.bold)
                };
                if !st.nowrap && !items.is_empty() && lw + trimmed > w + 0.01 {
                    self.finish_line(&mut items, &mut lw, &mut lh, &mut out, &mut cy, x, w, align);
                }
                if !st.pre && items.is_empty() && tok.trim().is_empty() {
                    continue;
                }
                items.push(LItem {
                    text: tok.to_string(),
                    px: st.font_px,
                    bold: st.bold,
                    color: st.color,
                    dx: lw,
                    w: full,
                    node: st.node,
                    bg: st.bg,
                    underline: st.underline,
                    strike: st.strike,
                    rise: st.rise,
                });
                lw += full;
                lh = lh.max(st.font_px * st.line_h);
            }
        }
        if !items.is_empty() {
            self.finish_line(&mut items, &mut lw, &mut lh, &mut out, &mut cy, x, w, align);
        }
        (out, cy - y)
    }

    pub(crate) fn build(
        &self,
        idx: usize,
        ps: &Style,
        x0: f32,
        y0: f32,
        avail: f32,
        pct_h: f32,
    ) -> LBox {
        self.build_w(idx, ps, x0, y0, avail, pct_h, None)
    }

    /// `force` fixes the content width (the row layout of inline blocks uses it).
    pub(crate) fn build_w(
        &self,
        idx: usize,
        ps: &Style,
        x0: f32,
        y0: f32,
        avail: f32,
        pct_h: f32,
        force: Option<f32>,
    ) -> LBox {
        let st = self.style_of(idx, ps);
        if self.dom.nodes[idx].tag == "table" {
            return self.build_table(idx, st, x0, y0, avail, pct_h);
        }
        let [mt, mr, mb, ml] = st.margin;
        let [pt, pr, pb, pl] = st.padding;
        if force.is_none() && st.inline_block && st.width.is_none() {
            // shrink to fit: lay it out at full width, then narrow it to what the content used
            let auto = (avail - ml - mr - pl - pr).max(0.0);
            let probe = self.build_w(idx, ps, x0, y0, avail, pct_h, Some(auto));
            let nat = natural_width(&probe, x0 + ml + pl);
            return self.build_w(idx, ps, x0, y0, avail, pct_h, Some(nat.ceil().min(auto)));
        }
        let (outer_w, content_w) = match (force, st.width) {
            (Some(cw), _) => (cw + pl + pr, cw),
            (None, Some(l)) => {
                let cw = l.px(avail).max(0.0);
                (cw + pl + pr, cw)
            }
            (None, None) => {
                let ow = (avail - ml - mr).max(0.0);
                (ow, (ow - pl - pr).max(0.0))
            }
        };
        let bx = if st.margin_auto && st.width.is_some() {
            x0 + ((avail - outer_w) / 2.0).max(0.0)
        } else {
            x0 + ml
        };
        let by = y0 + mt;
        let cx = bx + pl;
        let mut cy = by + pt;
        let child_pct_h = match st.height {
            Some(l) => l.px(pct_h),
            None => pct_h,
        };
        let mut items = Vec::new();
        let mut runs: Vec<(String, Style)> = Vec::new();
        let flush = |runs: &mut Vec<(String, Style)>, cy: &mut f32, items: &mut Vec<Item>| {
            if runs.iter().any(|(t, _)| !t.trim().is_empty() || t == "\n") {
                let (lines, h) = self.lines(runs, cx, *cy, content_w, st.align);
                *cy += h;
                items.extend(lines.into_iter().map(Item::Line));
            }
            runs.clear();
        };
        // consecutive `inline-block` children are laid out in rows, wrapping at the content width
        let mut group: Vec<usize> = Vec::new();
        let place = |group: &mut Vec<usize>, cy: &mut f32, items: &mut Vec<Item>| {
            if group.is_empty() {
                return;
            }
            let mut boxes: Vec<LBox> = group
                .drain(..)
                .map(|k| self.build_w(k, &st, 0.0, 0.0, content_w, child_pct_h, None))
                .collect();
            let occupied = |b: &LBox| b.st.margin[3] + b.rect[2] + b.st.margin[1];
            let mut i = 0;
            while i < boxes.len() {
                let (mut j, mut rw) = (i, 0.0f32);
                while j < boxes.len() {
                    let ow = occupied(&boxes[j]);
                    if j > i && rw + ow > content_w + 0.01 {
                        break;
                    }
                    rw += ow;
                    j += 1;
                }
                let off = match st.align {
                    1 => (content_w - rw) / 2.0,
                    2 => content_w - rw,
                    _ => 0.0,
                }
                    .max(0.0);
                let rh = boxes[i..j]
                    .iter()
                    .map(|b| b.st.margin[0] + b.rect[3] + b.st.margin[2])
                    .fold(0.0, f32::max);
                let mut x = cx + off;
                for b in &mut boxes[i..j] {
                    let ow = occupied(b);
                    shift(b, x, *cy);
                    x += ow;
                }
                *cy += rh;
                i = j;
            }
            items.extend(boxes.into_iter().map(Item::Block));
        };
        let tag = self.dom.nodes[idx].tag.as_str();
        // elements whose content is not laid out as children
        let no_kids = matches!(
            tag,
            "img"
                | "canvas"
                | "video"
                | "svg"
                | "iframe"
                | "embed"
                | "object"
                | "input"
                | "select"
                | "progress"
                | "meter"
                | "hr"
        );
        let closed_details = tag == "details" && self.dom.attr(idx, "open").is_none();
        let kids: &[usize] = if no_kids {
            &[]
        } else {
            &self.dom.nodes[idx].kids
        };
        for &k in kids {
            let kn = &self.dom.nodes[k];
            if closed_details && kn.tag != "summary" {
                continue;
            }
            if let Some(t) = &kn.text {
                place(&mut group, &mut cy, &mut items);
                let mut ts = st.clone();
                ts.bg = [0; 4];
                push_text(&mut runs, t, &ts);
                continue;
            }
            let cs = self.style_of(k, &st);
            if cs.none {
                continue;
            }
            if kn.tag == "br" {
                place(&mut group, &mut cy, &mut items);
                runs.push(("\n".into(), cs));
                continue;
            }
            if cs.inline_block {
                flush(&mut runs, &mut cy, &mut items);
                group.push(k);
                continue;
            }
            if cs.inline {
                place(&mut group, &mut cy, &mut items);
                self.inline_runs(k, &cs, &mut runs);
                continue;
            }
            place(&mut group, &mut cy, &mut items);
            flush(&mut runs, &mut cy, &mut items);
            let child = self.build(k, &st, cx, cy, content_w, child_pct_h);
            cy = child.rect[1] + child.rect[3] + child.mb;
            items.push(Item::Block(child));
        }
        if let Some((t, c)) = self.control_text(idx) {
            let mut ts = st.clone();
            ts.bg = [0; 4];
            if let Some(c) = c {
                ts.color = c;
            }
            push_text(&mut runs, &t, &ts);
        }
        place(&mut group, &mut cy, &mut items);
        flush(&mut runs, &mut cy, &mut items);
        let content_h = match st.height {
            Some(l) => l.px(pct_h),
            None if st.aspect > 0.0 => content_w * st.aspect,
            None => cy - (by + pt),
        };
        // a button with a fixed height keeps its label in the middle
        if matches!(tag, "button" | "input") && st.height.is_some() {
            let dy = ((content_h - (cy - (by + pt))) / 2.0).max(0.0);
            if dy > 0.0 {
                for it in &mut items {
                    match it {
                        Item::Block(c) => shift(c, 0.0, dy),
                        Item::Line(l) => l.y += dy,
                    }
                }
            }
        }
        let h = content_h + pt + pb;
        let marker = (tag == "li" && st.list != 0).then(|| self.marker_text(idx, st.list));
        LBox {
            rect: [bx, by, outer_w, h],
            mb,
            st,
            items,
            node: idx,
            marker,
        }
    }

    fn collect_rows(
        &self,
        parent: usize,
        pst: &Style,
        rows: &mut Vec<(usize, Style)>,
        caps: &mut Vec<usize>,
    ) {
        for &k in &self.dom.nodes[parent].kids {
            let kn = &self.dom.nodes[k];
            if kn.text.is_some() {
                continue;
            }
            match kn.tag.as_str() {
                "tr" => {
                    let s = self.style_of(k, pst);
                    if !s.none {
                        rows.push((k, s));
                    }
                }
                "thead" | "tbody" | "tfoot" => {
                    let s = self.style_of(k, pst);
                    if !s.none {
                        self.collect_rows(k, &s, rows, caps);
                    }
                }
                "caption" => caps.push(k),
                _ => {}
            }
        }
    }

    /// A `<table>`: cells on a grid (`colspan`, `rowspan`), column widths from the content
    /// (or `width`), rows as tall as their tallest cell.
    fn build_table(
        &self,
        idx: usize,
        st: Style,
        x0: f32,
        y0: f32,
        avail: f32,
        pct_h: f32,
    ) -> LBox {
        struct Cell {
            node: usize,
            st: Style,
            row: usize,
            col: usize,
            rs: usize,
            cs: usize,
        }
        let [mt, mr, mb, ml] = st.margin;
        let [pt, pr, pb, pl] = st.padding;
        let sp = st.spacing;
        let mut rows: Vec<(usize, Style)> = Vec::new();
        let mut caps: Vec<usize> = Vec::new();
        self.collect_rows(idx, &st, &mut rows, &mut caps);
        let span = |n: usize, name: &str| {
            self.dom
                .attr(n, name)
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 100)
        };
        let mut cells: Vec<Cell> = Vec::new();
        let mut occ: Vec<Vec<bool>> = vec![Vec::new(); rows.len()];
        for (r, (tr, rst)) in rows.iter().enumerate() {
            let mut c = 0;
            for &k in &self.dom.nodes[*tr].kids {
                let kn = &self.dom.nodes[k];
                if kn.text.is_some() || !(kn.tag == "td" || kn.tag == "th") {
                    continue;
                }
                let cst = self.style_of(k, rst);
                if cst.none {
                    continue;
                }
                while occ[r].get(c).copied().unwrap_or(false) {
                    c += 1;
                }
                let cs = span(k, "colspan");
                let rs = span(k, "rowspan").min(rows.len() - r);
                for rr in r..r + rs {
                    if occ[rr].len() < c + cs {
                        occ[rr].resize(c + cs, false);
                    }
                    for cc in c..c + cs {
                        occ[rr][cc] = true;
                    }
                }
                cells.push(Cell {
                    node: k,
                    st: cst,
                    row: r,
                    col: c,
                    rs,
                    cs,
                });
                c += cs;
            }
        }
        let n = cells.iter().map(|c| c.col + c.cs).max().unwrap_or(0);
        let inner_avail = (avail - ml - mr - pl - pr).max(0.0);
        // natural and minimal width of every cell
        const BIG: f32 = 20000.0;
        let mut nat = vec![0.0f32; n];
        let mut minw = vec![0.0f32; n];
        let mut pct: Vec<Option<f32>> = vec![None; n];
        let mut cell_nat = Vec::with_capacity(cells.len());
        let mut cell_min = Vec::with_capacity(cells.len());
        for c in &cells {
            let rst = &rows[c.row].1;
            let chrome = c.st.padding[1] + c.st.padding[3];
            let lx = c.st.margin[3] + c.st.padding[3];
            let wide = self.build_w(c.node, rst, 0.0, 0.0, BIG, pct_h, Some(BIG));
            let mut cn = (natural_width(&wide, lx) + chrome).min(inner_avail.max(1.0));
            let narrow = self.build_w(c.node, rst, 0.0, 0.0, 0.0, pct_h, Some(0.0));
            let mut cm = (natural_width(&narrow, lx) + chrome).min(cn);
            match c.st.width {
                Some(Len::Px(w)) => {
                    cn = w + chrome;
                    cm = cm.min(cn);
                }
                Some(Len::Pct(p)) if c.cs == 1 => {
                    pct[c.col] = Some(pct[c.col].map_or(p, |q| q.max(p)));
                }
                _ => {}
            }
            if c.st.nowrap {
                cm = cn;
            }
            cell_nat.push(cn);
            cell_min.push(cm);
        }
        for (i, c) in cells.iter().enumerate() {
            if c.cs == 1 {
                nat[c.col] = nat[c.col].max(cell_nat[i]);
                minw[c.col] = minw[c.col].max(cell_min[i]);
            }
        }
        let mut spanned: Vec<usize> = (0..cells.len()).filter(|&i| cells[i].cs > 1).collect();
        spanned.sort_by_key(|&i| cells[i].cs);
        for i in spanned {
            let c = &cells[i];
            for (arr, want) in [(&mut nat, cell_nat[i]), (&mut minw, cell_min[i])] {
                let have: f32 =
                    arr[c.col..c.col + c.cs].iter().sum::<f32>() + sp * (c.cs - 1) as f32;
                if want > have {
                    let add = (want - have) / c.cs as f32;
                    for v in &mut arr[c.col..c.col + c.cs] {
                        *v += add;
                    }
                }
            }
        }
        for c in 0..n {
            if minw[c] > nat[c] {
                nat[c] = minw[c];
            }
        }
        // column widths
        let gaps = sp * (n + 1) as f32;
        let nat_total: f32 = nat.iter().sum::<f32>() + gaps;
        let has_pct = pct.iter().any(|p| p.is_some());
        let target = match st.width {
            Some(l) => l.px(avail).max(0.0),
            None if has_pct => inner_avail,
            None => nat_total.min(inner_avail),
        };
        let room = (target - gaps).max(0.0);
        let mut w = nat.clone();
        let mut fixed = vec![false; n];
        let mut used = 0.0;
        for c in 0..n {
            if let Some(p) = pct[c] {
                w[c] = (room * p / 100.0).max(minw[c]);
                fixed[c] = true;
                used += w[c];
            }
        }
        let free: Vec<usize> = (0..n).filter(|&c| !fixed[c]).collect();
        let rest = (room - used).max(0.0);
        let sum_nat: f32 = free.iter().map(|&c| nat[c]).sum();
        let sum_min: f32 = free.iter().map(|&c| minw[c]).sum();
        if !free.is_empty() {
            if rest >= sum_nat {
                let extra = rest - sum_nat;
                for &c in &free {
                    w[c] = nat[c]
                        + if sum_nat > 0.0 {
                        extra * nat[c] / sum_nat
                    } else {
                        extra / free.len() as f32
                    };
                }
            } else if rest > sum_min && sum_nat > sum_min {
                let k = (rest - sum_min) / (sum_nat - sum_min);
                for &c in &free {
                    w[c] = minw[c] + (nat[c] - minw[c]) * k;
                }
            } else {
                for &c in &free {
                    w[c] = minw[c];
                }
            }
        }
        let content_w = w.iter().sum::<f32>() + gaps;
        let outer_w = content_w + pl + pr;
        let bx = if st.margin_auto {
            x0 + ((avail - outer_w) / 2.0).max(0.0)
        } else {
            x0 + ml
        };
        let by = y0 + mt;
        let cx = bx + pl;
        let cy = by + pt;
        // build every cell at its column width, then size the rows
        let mut built: Vec<LBox> = Vec::with_capacity(cells.len());
        for c in &cells {
            let cwid = w[c.col..c.col + c.cs].iter().sum::<f32>() + sp * (c.cs - 1) as f32;
            let chrome = c.st.padding[1] + c.st.padding[3];
            built.push(self.build_w(
                c.node,
                &rows[c.row].1,
                0.0,
                0.0,
                cwid,
                pct_h,
                Some((cwid - chrome).max(0.0)),
            ));
        }
        let mut rh = vec![0.0f32; rows.len()];
        for (i, c) in cells.iter().enumerate() {
            if c.rs == 1 {
                rh[c.row] = rh[c.row].max(built[i].rect[3]);
            }
        }
        for (r, (_, rst)) in rows.iter().enumerate() {
            if let Some(l) = rst.height {
                rh[r] = rh[r].max(l.px(pct_h));
            }
        }
        for (i, c) in cells.iter().enumerate() {
            if c.rs > 1 {
                let have: f32 =
                    rh[c.row..c.row + c.rs].iter().sum::<f32>() + sp * (c.rs - 1) as f32;
                if built[i].rect[3] > have {
                    rh[c.row + c.rs - 1] += built[i].rect[3] - have;
                }
            }
        }
        let mut items: Vec<Item> = Vec::new();
        let mut y = cy;
        for &cap in &caps {
            let child = self.build(cap, &st, cx, y, content_w, pct_h);
            y = child.rect[1] + child.rect[3] + child.mb;
            items.push(Item::Block(child));
        }
        y += sp;
        let mut ry = Vec::with_capacity(rows.len());
        for r in 0..rows.len() {
            ry.push(y);
            y += rh[r] + sp;
        }
        let n_rows = rows.len();
        let mut row_items: Vec<Vec<Item>> = (0..rows.len()).map(|_| Vec::new()).collect();
        for (c, mut b) in cells.iter().zip(built) {
            let height = rh[c.row..c.row + c.rs].iter().sum::<f32>() + sp * (c.rs - 1) as f32;
            let extra = (height - b.rect[3]).max(0.0);
            let dy = match c.st.valign {
                1 => extra / 2.0,
                2 => extra,
                _ => 0.0,
            };
            if dy > 0.0 {
                for it in &mut b.items {
                    match it {
                        Item::Block(k) => shift(k, 0.0, dy),
                        Item::Line(l) => l.y += dy,
                    }
                }
            }
            b.rect[3] = b.rect[3].max(height);
            if st.collapse {
                // neighbouring cells share one line: only the last column / row draws its far edge
                if c.col + c.cs < n {
                    b.st.border[1] = 0.0;
                }
                if c.row + c.rs < n_rows {
                    b.st.border[2] = 0.0;
                }
            }
            let x = cx + sp + w[..c.col].iter().sum::<f32>() + sp * c.col as f32;
            shift(&mut b, x, ry[c.row]);
            row_items[c.row].push(Item::Block(b));
        }
        for (r, ((tr, rst), its)) in rows.into_iter().zip(row_items).enumerate() {
            items.push(Item::Block(LBox {
                rect: [cx + sp, ry[r], (content_w - 2.0 * sp).max(0.0), rh[r]],
                mb: 0.0,
                st: rst,
                items: its,
                node: tr,
                marker: None,
            }));
        }
        let used_h = y - (by + pt);
        let content_h = match st.height {
            Some(l) => used_h.max(l.px(pct_h)),
            None => used_h,
        };
        LBox {
            rect: [bx, by, outer_w, content_h + pt + pb],
            mb,
            st,
            items,
            node: idx,
            marker: None,
        }
    }
}
