//! The friend locator: where on the edge of the screen to point at a friend who is out of view,
//! and what to write next to the arrow. Pure geometry in CSS pixels, drawn by `web.rs`.

/// An arrow on the screen edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub x: f32,
    pub y: f32,
    /// Degrees clockwise from pointing right, for a glyph that points right when not rotated.
    pub angle_deg: f32,
}

/// Where to put the arrow for a friend at `target` (screen position, CSS px) on a screen of `size`.
/// `None` while the friend is inside the screen less `pad` on each side: they are in view and
/// their name tag does the job. Otherwise the arrow sits where the line from the middle of the
/// screen to the friend crosses that inner rectangle, and points along the line.
pub fn edge_marker(target: (f32, f32), size: (f32, f32), pad: (f32, f32)) -> Option<Marker> {
    let (cx, cy) = (size.0 / 2.0, size.1 / 2.0);
    let (dx, dy) = (target.0 - cx, target.1 - cy);
    // A screen smaller than the padding still gets a marker, in the middle.
    let (hx, hy) = ((cx - pad.0).max(1.0), (cy - pad.1).max(1.0));
    if dx.abs() <= hx && dy.abs() <= hy {
        return None;
    }
    let t = (hx / dx.abs().max(f32::EPSILON)).min(hy / dy.abs().max(f32::EPSILON));
    Some(Marker { x: cx + dx * t, y: cy + dy * t, angle_deg: dy.atan2(dx).to_degrees() })
}

/// "42m", or "1.2km" from a thousand blocks on.
pub fn distance_label(blocks: f32) -> String {
    if blocks >= 1000.0 {
        format!("{:.1}km", blocks / 1000.0)
    } else {
        format!("{}m", blocks.round() as i32)
    }
}

/// With a jetpack, up and down matter: an arrow for a friend clearly above or below. `dz` is
/// their height minus ours, in blocks.
pub fn height_hint(dz: f32) -> Option<char> {
    const CLEARLY: f32 = 3.0;
    if dz >= CLEARLY {
        Some('▲')
    } else if dz <= -CLEARLY {
        Some('▼')
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (800.0, 600.0);
    const PAD: (f32, f32) = (60.0, 40.0);

    #[test]
    fn a_friend_in_view_needs_no_arrow() {
        assert_eq!(edge_marker((400.0, 300.0), SIZE, PAD), None);
        assert_eq!(edge_marker((100.0, 100.0), SIZE, PAD), None);
        assert_eq!(edge_marker((740.0, 560.0), SIZE, PAD), None, "on the inner rectangle itself still counts as in view");
    }

    #[test]
    fn straight_out_the_right_edge() {
        let m = edge_marker((5000.0, 300.0), SIZE, PAD).unwrap();
        assert_eq!((m.x, m.y), (740.0, 300.0));
        assert_eq!(m.angle_deg, 0.0);
    }

    #[test]
    fn straight_up_points_up() {
        let m = edge_marker((400.0, -9000.0), SIZE, PAD).unwrap();
        assert_eq!((m.x, m.y), (400.0, 40.0));
        assert_eq!(m.angle_deg, -90.0);
    }

    #[test]
    fn down_left_lands_on_the_edge_the_line_hits_first() {
        // Slope 1 from the middle (400, 300): the bottom edge (y = 560, 260 away) comes before the left (x = 60, 340 away).
        let m = edge_marker((-600.0, 1300.0), SIZE, PAD).unwrap();
        assert!((m.x - 140.0).abs() < 0.01 && (m.y - 560.0).abs() < 0.01, "{m:?}");
        assert!((m.angle_deg - 135.0).abs() < 0.01);
    }

    #[test]
    fn the_arrow_always_stays_inside_the_padded_screen() {
        for (tx, ty) in [(-300.0, 20.0), (1200.0, 900.0), (400.0, 700.0), (-50.0, -50.0), (801.0, 300.0)] {
            if let Some(m) = edge_marker((tx, ty), SIZE, PAD) {
                assert!(m.x >= PAD.0 - 0.01 && m.x <= SIZE.0 - PAD.0 + 0.01, "x {} for {tx},{ty}", m.x);
                assert!(m.y >= PAD.1 - 0.01 && m.y <= SIZE.1 - PAD.1 + 0.01, "y {} for {tx},{ty}", m.y);
            }
        }
    }

    #[test]
    fn a_tiny_screen_does_not_divide_by_zero() {
        let m = edge_marker((500.0, 500.0), (50.0, 50.0), PAD).unwrap();
        assert!(m.x.is_finite() && m.y.is_finite());
    }

    #[test]
    fn labels() {
        assert_eq!(distance_label(0.4), "0m");
        assert_eq!(distance_label(42.6), "43m");
        assert_eq!(distance_label(1234.0), "1.2km");
        assert_eq!(height_hint(2.9), None);
        assert_eq!(height_hint(3.0), Some('▲'));
        assert_eq!(height_hint(-12.0), Some('▼'));
    }
}
