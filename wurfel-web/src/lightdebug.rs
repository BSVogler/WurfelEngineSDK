//! Light engine debug readout, ported from the text part of the Java `LightEngine.render`: where the
//! sun and the moon are (latitude = height, longitude = azimuth), how strong they shine and the
//! resulting ambient colour. Pure text, so it is unit tested natively; `web.rs` shows it next to the
//! network overlay when the debug display is on.

use wurfel_sim::light::{GlobalLightSource, LightEngine};

/// Radius of the sphere in the diagram, in CSS pixels (the Java `size` was 500 on a full screen).
pub const DIAGRAM_RADIUS: f32 = 60.0;
/// Side of the square canvas that holds the diagram.
pub const DIAGRAM_SIZE: f32 = 220.0;

fn rgb(c: glam::Vec3) -> String {
    format!("({:.2}, {:.2}, {:.2})", c.x, c.y, c.z)
}

fn body(name: &str, source: &GlobalLightSource, out: &mut String) {
    let n = source.normal_game();
    out.push_str(&format!(
        "{name:<5} lat {:>6.1}°  long {:>6.1}°  power {:>3.0}%{}\n      normal ({:.2}, {:.2}, {:.2})  colour {}\n",
        source.height(),
        source.azimuth(),
        source.power() * 100.0,
        if n.z > 0.0 { "" } else { "  (below horizon)" },
        n.x,
        n.y,
        n.z,
        rgb(source.light()),
    ));
}

/// The overlay's text for this frame.
pub fn format_report(engine: &LightEngine) -> String {
    let state = engine.state();
    let mut out = format!("LIGHT  time of day {:.2}  night {:.0}%\n", state.time_of_day, state.night_mix * 100.0);
    body("SUN", engine.sun(), &mut out);
    match engine.moon() {
        Some(moon) => body("MOON", moon, &mut out),
        None => out.push_str("MOON  none\n"),
    }
    out.push_str(&format!(
        "ambient {}\nsun faces L/T/R {:.2} {:.2} {:.2}  spec {:.2}",
        rgb(state.ambient),
        state.sun_faces[0],
        state.sun_faces[1],
        state.sun_faces[2],
        state.sun_spec,
    ));
    out
}

/// One coloured line of the diagram, in pixels from the centre of the sphere, y down.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub color: &'static str,
}

/// A horizontal circle around the sphere seen at the diagram's slant: an ellipse (`ry` is half of
/// `rx`) with its centre `centre_y` pixels from the middle (negative is up).
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub centre_y: f32,
    pub rx: f32,
    pub ry: f32,
    pub color: &'static str,
}

/// The sun/moon circle of the Java `LightEngine.render`, as lines and labels.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Diagram {
    pub radius: f32,
    /// The circles of latitude on the sphere, where the sun and the moon touch it.
    pub rings: Vec<Ring>,
    pub segments: Vec<Segment>,
    pub labels: Vec<(&'static str, (f32, f32))>,
}

/// Where a light with this normal (`GlobalLightSource::normal_game`) stands on the sphere: the
/// ground point under it and the point in the air. Java's y points up, ours down.
fn spots(n: glam::Vec3, r: f32) -> ((f32, f32), (f32, f32)) {
    let ground = (r * n.x, r / 2.0 * n.y);
    let air = (r * n.x, -r * (-n.y / 2.0 + n.z));
    (ground, air)
}

/// The circle of latitude of a light with this normal: all points of the sphere as high as the
/// light, so the light touches the sphere on it.
fn ring(n: glam::Vec3, r: f32, color: &'static str) -> Ring {
    let rx = r * (1.0 - n.z * n.z).max(0.0).sqrt();
    Ring { centre_y: -r * n.z, rx, ry: rx / 2.0, color }
}

/// The Java drawing: the red line is the longitude on the ground, the magenta ring the latitude (a
/// line in Java, a circle around the sphere here), the grey one connects the ground point to the light when it is above the horizon, and the yellow
/// (sun) or blue (moon) line points at the light itself.
pub fn diagram(sun: glam::Vec3, moon: Option<glam::Vec3>, r: f32) -> Diagram {
    let mut d = Diagram { radius: r, ..Diagram::default() };
    let origin = (0.0, 0.0);
    let (ground, air) = spots(sun, r);
    d.segments.push(Segment { from: origin, to: ground, color: "#ff0000" });
    if sun.z > 0.0 {
        d.segments.push(Segment { from: ground, to: air, color: "#888888" });
    }
    d.rings.push(ring(sun, r, "#ff00ff"));
    d.segments.push(Segment { from: origin, to: air, color: "#ffff00" });
    d.labels.push(("SUN", air));
    if let Some(moon) = moon {
        let (ground, air) = spots(moon, r);
        if moon.z > 0.0 {
            d.segments.push(Segment { from: ground, to: air, color: "#888888" });
        }
        d.rings.push(ring(moon, r, "#4a6bff"));
        d.segments.push(Segment { from: origin, to: air, color: "#4a6bff" });
        d.labels.push(("MOON", air));
    }
    d
}

/// The diagram for the engine's current sun and moon.
pub fn engine_diagram(engine: &LightEngine) -> Diagram {
    diagram(engine.sun().normal_game(), engine.moon().map(|m| m.normal_game()), DIAGRAM_RADIUS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_report_shows_sun_and_moon() {
        let mut engine = LightEngine::new(0, 0.0);
        engine.set_to_noon();
        let text = format_report(&engine);
        assert!(text.contains("SUN"), "{text}");
        assert!(text.contains("MOON"), "{text}");
        assert!(text.contains("ambient"), "{text}");
        assert!(!text.contains("NaN"), "{text}");
    }

    #[test]
    fn night_puts_sun_below_horizon() {
        let mut engine = LightEngine::new(0, 0.0);
        engine.set_to_night();
        assert!(format_report(&engine).contains("below horizon"));
    }

    #[test]
    fn diagram_marks_the_sun_above_the_ground_point() {
        // Sun straight overhead: its line goes straight up from the centre.
        let d = diagram(glam::Vec3::Z, None, 60.0);
        let sun = d.segments.last().unwrap();
        assert_eq!(sun.color, "#ffff00");
        assert!(sun.to.0.abs() < 1e-4 && (sun.to.1 + 60.0).abs() < 1e-4, "{sun:?}");
        assert_eq!(d.labels.len(), 1);
    }

    #[test]
    fn diagram_has_no_connection_line_below_the_horizon() {
        let d = diagram(glam::Vec3::new(0.0, 0.0, -1.0), Some(glam::Vec3::Z), 60.0);
        let greys = d.segments.iter().filter(|s| s.color == "#888888").count();
        assert_eq!(greys, 1, "only the moon is up");
        assert_eq!(d.labels.len(), 2);
    }

    #[test]
    fn the_sun_touches_its_latitude_ring() {
        let n = glam::Vec3::new(-0.6, 0.48, 0.64).normalize();
        let d = diagram(n, None, 60.0);
        let ring = &d.rings[0];
        let sun = d.segments.last().unwrap().to;
        // The marker is on the ellipse: ((x / rx)^2 + ((y - cy) / ry)^2 = 1.
        let on = (sun.0 / ring.rx).powi(2) + ((sun.1 - ring.centre_y) / ring.ry).powi(2);
        assert!((on - 1.0).abs() < 1e-3, "{on}");
    }
}
