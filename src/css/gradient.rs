//! Parsing of CSS gradient images: `linear-gradient`, `radial-gradient`,
//! `conic-gradient` and the `repeating-` radial/conic variants.

use crate::alloc_prelude::*;
use crate::graphics::{Color, ConicGradient, Gradient, LinearGradient, RadialExtent, RadialGradient, RadialShape};
use super::parser::parse_hex_color;

/// Parse a gradient image such as `conic-gradient(from 90deg, red, blue)`.
/// Returns `None` if `val` is not a gradient or is malformed.
pub fn parse_gradient(val: &str) -> Option<Gradient> {
    let val = val.trim();
    let open = val.find('(')?;
    let inner = val[open + 1..].strip_suffix(')')?;
    let args = split_top_level(inner, ',');
    match val[..open].trim() {
        "linear-gradient" => parse_linear(&args).map(Gradient::Linear),
        "radial-gradient" => parse_radial(&args, false).map(Gradient::Radial),
        "repeating-radial-gradient" => parse_radial(&args, true).map(Gradient::Radial),
        "conic-gradient" => parse_conic(&args, false).map(Gradient::Conic),
        "repeating-conic-gradient" => parse_conic(&args, true).map(Gradient::Conic),
        _ => None,
    }
}

/// Blend two gradients for keyframe animation. Gradients of the same kind
/// with the same number of stops interpolate their angle, center, stop
/// colors and positions; anything else switches at the halfway point.
/// Whether [`interpolate_gradient`] blends `a` and `b` continuously: the
/// same kind and shape, with as many stops. Others flip halfway.
pub fn gradients_interpolate(a: &Gradient, b: &Gradient) -> bool {
    match (a, b) {
        (Gradient::Linear(x), Gradient::Linear(y)) => x.stops.len() == y.stops.len(),
        (Gradient::Radial(x), Gradient::Radial(y)) => x.stops.len() == y.stops.len() && x.shape == y.shape && x.extent == y.extent,
        (Gradient::Conic(x), Gradient::Conic(y)) => x.stops.len() == y.stops.len(),
        _ => false,
    }
}

pub fn interpolate_gradient(a: &Gradient, b: &Gradient, t: f32) -> Gradient {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let lerp_pt = |p: (f32, f32), q: (f32, f32)| (lerp(p.0, q.0), lerp(p.1, q.1));
    let lerp_stops = |sa: &[(Color, f32)], sb: &[(Color, f32)]| -> Vec<(Color, f32)> {
        sa.iter()
            .zip(sb)
            .map(|(&(ca, pa), &(cb, pb))| {
                let ch = |x: u8, y: u8| (lerp(x as f32, y as f32) + 0.5) as u8;
                (Color::new(ch(ca.r, cb.r), ch(ca.g, cb.g), ch(ca.b, cb.b), ch(ca.a, cb.a)), lerp(pa, pb))
            })
            .collect()
    };
    match (a, b) {
        (Gradient::Linear(x), Gradient::Linear(y)) if x.stops.len() == y.stops.len() => Gradient::Linear(LinearGradient {
            angle: lerp(x.angle, y.angle),
            stops: lerp_stops(&x.stops, &y.stops).into(),
        }),
        (Gradient::Radial(x), Gradient::Radial(y))
            if x.stops.len() == y.stops.len() && x.shape == y.shape && x.extent == y.extent =>
        {
            Gradient::Radial(RadialGradient {
                center: lerp_pt(x.center, y.center),
                stops: lerp_stops(&x.stops, &y.stops).into(),
                ..x.clone()
            })
        }
        (Gradient::Conic(x), Gradient::Conic(y)) if x.stops.len() == y.stops.len() => Gradient::Conic(ConicGradient {
            from: lerp(x.from, y.from),
            center: lerp_pt(x.center, y.center),
            stops: lerp_stops(&x.stops, &y.stops).into(),
            repeating: x.repeating,
        }),
        _ => if t < 0.5 { a.clone() } else { b.clone() },
    }
}

fn parse_linear(args: &[&str]) -> Option<LinearGradient> {
    let (angle, stop_args) = match args.first().map(|a| a.trim()) {
        Some(first) if first.starts_with("to ") => {
            let words: Vec<&str> = first[3..].split_whitespace().collect();
            let has = |w: &str| words.contains(&w);
            let angle = match (has("top"), has("bottom"), has("left"), has("right")) {
                (true, _, true, _) => 315.0,
                (true, _, _, true) => 45.0,
                (_, true, true, _) => 225.0,
                (_, true, _, true) => 135.0,
                (true, _, _, _) => 0.0,
                (_, _, _, true) => 90.0,
                (_, _, true, _) => 270.0,
                _ => 180.0,
            };
            (angle, &args[1..])
        }
        Some(first) => match parse_angle(first) {
            Some(turns) => (turns * 360.0, &args[1..]),
            None => (180.0, args),
        },
        None => return None,
    };
    let stops = parse_stops(stop_args, false)?;
    Some(LinearGradient { angle, stops: stops.into() })
}

fn parse_radial(args: &[&str], repeating: bool) -> Option<RadialGradient> {
    let mut shape = RadialShape::Ellipse;
    let mut extent = RadialExtent::FarthestCorner;
    let mut center = (0.5, 0.5);

    let mut stop_args = args;
    if let Some(first) = args.first() {
        let mut words = first.split_whitespace();
        let mut is_prelude = false;
        while let Some(word) = words.next() {
            match word {
                "circle" => shape = RadialShape::Circle,
                "ellipse" => shape = RadialShape::Ellipse,
                "closest-side" => extent = RadialExtent::ClosestSide,
                "farthest-side" => extent = RadialExtent::FarthestSide,
                "closest-corner" => extent = RadialExtent::ClosestCorner,
                "farthest-corner" => extent = RadialExtent::FarthestCorner,
                "at" => center = parse_position(words.by_ref()),
                _ => {
                    is_prelude = false;
                    break;
                }
            }
            is_prelude = true;
        }
        if is_prelude {
            stop_args = &args[1..];
        }
    }

    let stops = parse_stops(stop_args, false)?;
    Some(RadialGradient { shape, extent, center, stops: stops.into(), repeating })
}

fn parse_conic(args: &[&str], repeating: bool) -> Option<ConicGradient> {
    let mut from = 0.0;
    let mut center = (0.5, 0.5);

    let mut stop_args = args;
    if let Some(first) = args.first() {
        let first = first.trim();
        if first.starts_with("from ") || first.starts_with("at ") {
            let mut words = first.split_whitespace();
            while let Some(word) = words.next() {
                match word {
                    "from" => from = parse_angle(words.next()?)? * 360.0,
                    "at" => center = parse_position(words.by_ref()),
                    _ => return None,
                }
            }
            stop_args = &args[1..];
        }
    }

    let stops = parse_stops(stop_args, true)?;
    Some(ConicGradient { from, center, stops: stops.into(), repeating })
}

/// Parse `<position>` keywords/percentages following `at` into box fractions.
fn parse_position<'a>(words: impl Iterator<Item = &'a str>) -> (f32, f32) {
    let mut x = None;
    let mut y = None;
    for word in words {
        match word {
            "left" => x = Some(0.0),
            "right" => x = Some(1.0),
            "top" => y = Some(0.0),
            "bottom" => y = Some(1.0),
            "center" => {}
            _ => {
                if let Some(p) = word.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
                    if x.is_none() {
                        x = Some(p / 100.0);
                    } else {
                        y = Some(p / 100.0);
                    }
                }
            }
        }
    }
    (x.unwrap_or(0.5), y.unwrap_or(0.5))
}

/// Parse an angle (`deg`, `turn`, `rad`, `grad`, or unitless `0`) as turns.
fn parse_angle(s: &str) -> Option<f32> {
    let s = s.trim();
    let (num, per_turn) = if let Some(n) = s.strip_suffix("deg") {
        (n, 360.0)
    } else if let Some(n) = s.strip_suffix("grad") {
        (n, 400.0)
    } else if let Some(n) = s.strip_suffix("rad") {
        (n, core::f32::consts::TAU)
    } else if let Some(n) = s.strip_suffix("turn") {
        (n, 1.0)
    } else if s == "0" {
        return Some(0.0);
    } else {
        return None;
    };
    Some(num.trim().parse::<f32>().ok()? / per_turn)
}

/// Parse a stop position as a fraction of the gradient line. Conic gradients
/// also accept angles; other units are unsupported.
fn parse_stop_position(s: &str, conic: bool) -> Option<f32> {
    if let Some(p) = s.strip_suffix('%') {
        return Some(p.trim().parse::<f32>().ok()? / 100.0);
    }
    if s == "0" {
        return Some(0.0);
    }
    if conic { parse_angle(s) } else { None }
}

/// Parse color stops (`color [pos [pos]]`) and resolve omitted positions the
/// way CSS does: first/last default to 0/1, positions never go backwards, and
/// runs of unpositioned stops are spread evenly between their neighbours.
fn parse_stops(args: &[&str], conic: bool) -> Option<Vec<(Color, f32)>> {
    let mut stops: Vec<(Color, Option<f32>)> = Vec::new();
    for arg in args {
        let parts = split_top_level(arg, ' ');
        let (color, positions) = parts.split_first()?;
        let color = parse_hex_color(color)?;
        match positions {
            [] => stops.push((color, None)),
            [p] => stops.push((color, Some(parse_stop_position(p, conic)?))),
            [p1, p2] => {
                stops.push((color, Some(parse_stop_position(p1, conic)?)));
                stops.push((color, Some(parse_stop_position(p2, conic)?)));
            }
            _ => return None,
        }
    }
    if stops.is_empty() {
        return None;
    }

    let last = stops.len() - 1;
    if stops[0].1.is_none() {
        stops[0].1 = Some(0.0);
    }
    if stops[last].1.is_none() {
        stops[last].1 = Some(if last == 0 { 0.0 } else { 1.0 });
    }

    let mut max_so_far = f32::MIN;
    for stop in stops.iter_mut() {
        if let Some(p) = stop.1.as_mut() {
            if *p < max_so_far {
                *p = max_so_far;
            }
            max_so_far = *p;
        }
    }

    let mut i = 0;
    while i < stops.len() {
        if stops[i].1.is_some() {
            i += 1;
            continue;
        }
        let run_start = i;
        while stops[i].1.is_none() {
            i += 1;
        }
        let before = stops[run_start - 1].1.unwrap();
        let after = stops[i].1.unwrap();
        let n = (i - run_start + 1) as f32;
        for (k, stop) in stops[run_start..i].iter_mut().enumerate() {
            stop.1 = Some(before + (after - before) * (k + 1) as f32 / n);
        }
    }

    Some(stops.into_iter().map(|(c, p)| (c, p.unwrap())).collect())
}

/// Split on `sep` outside of parentheses, trimming and dropping empty pieces.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

    fn conic(val: &str) -> ConicGradient {
        match parse_gradient(val) {
            Some(Gradient::Conic(c)) => c,
            other => panic!("expected conic gradient, got {:?}", other),
        }
    }

    #[test]
    fn linear_keeps_rgba_stops_whole() {
        let Some(Gradient::Linear(g)) = parse_gradient("linear-gradient(90deg, rgba(255, 0, 0, 0.5) 0%, #0000ff 100%)") else {
            panic!("not linear");
        };
        assert_eq!(g.angle, 90.0);
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].0.a, 127);
        assert_eq!(g.stops[1], (BLUE, 1.0));
    }

    #[test]
    fn linear_side_keywords() {
        let Some(Gradient::Linear(g)) = parse_gradient("linear-gradient(to right, red, blue)") else {
            panic!("not linear");
        };
        assert_eq!(g.angle, 90.0);
        let Some(Gradient::Linear(g)) = parse_gradient("linear-gradient(red, blue)") else {
            panic!("not linear");
        };
        assert_eq!(g.angle, 180.0);
    }

    #[test]
    fn conic_from_angle_and_hard_stops() {
        let g = conic("conic-gradient(from 225deg, red 0deg 90deg, transparent 90deg)");
        assert_eq!(g.from, 225.0);
        assert_eq!(g.center, (0.5, 0.5));
        assert!(!g.repeating);
        assert_eq!(g.stops.len(), 3);
        assert_eq!(g.stops[0], (RED, 0.0));
        assert_eq!(g.stops[1], (RED, 0.25));
        assert_eq!(g.stops[2].1, 0.25);
    }

    #[test]
    fn conic_at_position_and_repeating() {
        let g = conic("repeating-conic-gradient(from 0.5turn at 25% 75%, red 0 10%, blue 10% 20%)");
        assert_eq!(g.from, 180.0);
        assert_eq!(g.center, (0.25, 0.75));
        assert!(g.repeating);
        assert_eq!(g.stops.len(), 4);
    }

    #[test]
    fn unpositioned_stops_are_spread_evenly() {
        let g = conic("conic-gradient(red, blue, red, blue, red)");
        let positions: Vec<f32> = g.stops.iter().map(|s| s.1).collect();
        assert_eq!(positions, [0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn stop_positions_never_go_backwards() {
        let g = conic("conic-gradient(red 50%, blue 20%)");
        assert_eq!(g.stops[1].1, 0.5);
    }

    #[test]
    fn radial_prelude() {
        let Some(Gradient::Radial(g)) = parse_gradient("radial-gradient(circle closest-side at left top, red, blue)") else {
            panic!("not radial");
        };
        assert_eq!(g.shape, RadialShape::Circle);
        assert_eq!(g.extent, RadialExtent::ClosestSide);
        assert_eq!(g.center, (0.0, 0.0));

        let Some(Gradient::Radial(g)) = parse_gradient("radial-gradient(red, blue)") else {
            panic!("not radial");
        };
        assert_eq!(g.shape, RadialShape::Ellipse);
        assert_eq!(g.extent, RadialExtent::FarthestCorner);
        assert_eq!(g.stops.len(), 2);
    }

    #[test]
    fn interpolates_matching_gradients() {
        let a = parse_gradient("conic-gradient(from 0deg, red 0%, blue 50%)").unwrap();
        let b = parse_gradient("conic-gradient(from 360deg, blue 0%, red 100%)").unwrap();
        let Gradient::Conic(mid) = interpolate_gradient(&a, &b, 0.5) else {
            panic!("not conic");
        };
        assert_eq!(mid.from, 180.0);
        assert_eq!(mid.stops[1].1, 0.75);
        assert_eq!(mid.stops[0].0, Color::new(128, 0, 128, 255));
    }

    #[test]
    fn mismatched_gradients_switch_halfway() {
        let a = parse_gradient("conic-gradient(red, blue)").unwrap();
        let b = parse_gradient("linear-gradient(red, blue)").unwrap();
        assert_eq!(interpolate_gradient(&a, &b, 0.4), a);
        assert_eq!(interpolate_gradient(&a, &b, 0.6), b);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_gradient("conic-gradient(from nowhere, red)").is_none());
        assert!(parse_gradient("conic-gradient()").is_none());
        assert!(parse_gradient("url(foo.png)").is_none());
        assert!(parse_gradient("radial-gradient(red 10px, blue)").is_none());
    }
}
