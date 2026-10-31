//! Hyprland color/gradient text <-> (r, g, b, a) floats.

pub type Rgba = [f64; 4];

const GREY: Rgba = [0.5, 0.5, 0.5, 1.0];

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn hex_bytes(h: &str) -> Option<Rgba> {
    if !is_hex(h) || (h.len() != 6 && h.len() != 8) {
        return None;
    }
    let b = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.0)
    };
    Some([b(0)?, b(2)?, b(4)?, if h.len() == 8 { b(6)? } else { 1.0 }])
}

pub fn parse_color_or(text: &str, fallback: Rgba) -> Rgba {
    parse_color(text).unwrap_or(fallback)
}

pub fn parse_color(text: &str) -> Option<Rgba> {
    let t = text.trim();
    if let Some(h) = t.strip_prefix("0x") {
        if h.len() == 8 && is_hex(h) {
            let [a, r, g, b] = hex_bytes(h)?;
            return Some([r, g, b, a]);
        }
    }
    let inner = t
        .strip_prefix("rgba(")
        .or_else(|| t.strip_prefix("rgb("))
        .and_then(|s| s.strip_suffix(')'));
    if let Some(inner) = inner {
        let inner = inner.trim();
        if inner.contains(',') {
            let parts: Vec<f64> = inner
                .split(',')
                .map(|p| p.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .ok()?;
            if parts.len() < 3 {
                return None;
            }
            let a = parts.get(3).copied().unwrap_or(1.0);
            return Some([
                parts[0] / 255.0,
                parts[1] / 255.0,
                parts[2] / 255.0,
                if a <= 1.0 { a } else { a / 255.0 },
            ]);
        }
        return hex_bytes(inner);
    }
    hex_bytes(t.trim_start_matches('#'))
}

fn byte(c: f64) -> u8 {
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn format_color(c: Rgba) -> String {
    format!(
        "rgba({:02x}{:02x}{:02x}{:02x})",
        byte(c[0]),
        byte(c[1]),
        byte(c[2]),
        byte(c[3])
    )
}

/// Returns (colors, angle in degrees).
pub fn parse_gradient(text: &str) -> (Vec<Rgba>, i32) {
    let mut cols = vec![];
    let mut angle = 0;
    // tokens: rgba(...) may contain spaces after commas, so split on ')' boundaries first
    let mut rest = text.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.starts_with("rgb") {
            if let Some(end) = rest.find(')') {
                if let Some(c) = parse_color(&rest[..=end]) {
                    cols.push(c);
                }
                rest = &rest[end + 1..];
                continue;
            }
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let tok = &rest[..end];
        if let Some(d) = tok.strip_suffix("deg") {
            if let Ok(v) = d.parse::<f64>() {
                angle = v as i32;
            }
        } else if tok.starts_with("0x") {
            if let Some(c) = parse_color(tok) {
                cols.push(c);
            }
        }
        rest = &rest[end..];
    }
    if cols.is_empty() {
        cols.push(GREY);
    }
    (cols, angle)
}

pub fn format_gradient(cols: &[Rgba], angle: i32) -> String {
    let mut s = cols
        .iter()
        .map(|c| format_color(*c))
        .collect::<Vec<_>>()
        .join(" ");
    if cols.len() > 1 {
        s.push_str(&format!(" {angle}deg"));
    }
    s
}

pub fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

pub fn to_gdk(c: Rgba) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::new(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32)
}

pub fn from_gdk(c: &gtk::gdk::RGBA) -> Rgba {
    [
        c.red() as f64,
        c.green() as f64,
        c.blue() as f64,
        c.alpha() as f64,
    ]
}

/// Linear gradient across a box at a Hyprland angle (0deg = left to right, CCW).
pub fn gradient_pattern(
    cols: &[Rgba],
    angle: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> gtk::cairo::LinearGradient {
    let a = angle.to_radians();
    let (dx, dy) = (a.cos(), -a.sin());
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let half = dx.abs() * w / 2.0 + dy.abs() * h / 2.0;
    let pat = gtk::cairo::LinearGradient::new(
        cx - dx * half,
        cy - dy * half,
        cx + dx * half,
        cy + dy * half,
    );
    let n = cols.len();
    for (i, c) in cols.iter().enumerate() {
        let off = if n > 1 {
            i as f64 / (n - 1) as f64
        } else {
            0.0
        };
        pat.add_color_stop_rgba(off, c[0], c[1], c[2], c[3]);
    }
    if n == 1 {
        let c = cols[0];
        pat.add_color_stop_rgba(1.0, c[0], c[1], c[2], c[3]);
    }
    pat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let (c, a) = parse_gradient("rgba(33ccffee) rgba(00ff99ee) 45deg");
        assert_eq!(c.len(), 2);
        assert_eq!(a, 45);
        assert_eq!(
            format_gradient(&c, a),
            "rgba(33ccffee) rgba(00ff99ee) 45deg"
        );
        assert_eq!(
            format_color(parse_color("0xff112233").unwrap()),
            "rgba(112233ff)"
        );
        assert_eq!(
            format_color(parse_color("rgb(10, 20, 30)").unwrap()),
            "rgba(0a141eff)"
        );
    }
}
