//! The Kestrel logo, drawn in code: a kestrel's head with its dark "tear mark" stripe, rufous on a dark rounded
//! square. Shapes are on a 100-unit grid and drawn with 1-pixel anti-aliasing at any size. Shared by the app
//! (window, taskbar, tray, sidebar) and build.rs (the exe's icon resource), so it uses only the standard library.

/// Straight-alpha RGBA pixels of the logo at `n` x `n`. Without `tile`, only the bird is drawn and its eye and
/// stripe are cut out, so whatever is behind shows through (the sidebar mark).
pub fn rgba(n: usize, tile: bool) -> Vec<u8> {
    type P = (f32, f32);
    enum Shape {
        Circle(P, f32),
        Poly(&'static [P]),
    }
    // Signed distance (negative inside) from point p to a shape, in grid units.
    fn dist(s: &Shape, (x, y): P) -> f32 {
        match s {
            Shape::Circle((cx, cy), r) => (x - cx).hypot(y - cy) - r,
            Shape::Poly(pts) => {
                let (mut d, mut inside) = (f32::MAX, false);
                for (i, &(ax, ay)) in pts.iter().enumerate() {
                    let (bx, by) = pts[(i + 1) % pts.len()];
                    let (dx, dy) = (bx - ax, by - ay);
                    let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                    d = d.min((x - ax - t * dx).hypot(y - ay - t * dy));
                    if (ay > y) != (by > y) && x < ax + (y - ay) * dx / dy {
                        inside = !inside;
                    }
                }
                if inside {
                    -d
                } else {
                    d
                }
            }
        }
    }
    const TILE: [f32; 3] = [30.0; 3];
    const RUFOUS: [f32; 3] = [200.0, 105.0, 58.0];
    const BEAK: [f32; 3] = [242.0, 193.0, 78.0];
    // Painted in order over the dark tile: head (its curves flattened to points), beak, eye, tear stripe.
    let layers = [
        (
            Shape::Poly(&[
                (26.0, 80.0),
                (25.3, 71.7),
                (25.5, 63.6),
                (26.7, 56.0),
                (28.8, 48.9),
                (31.8, 42.5),
                (35.6, 36.9),
                (40.4, 32.4),
                (46.1, 28.9),
                (52.6, 26.8),
                (60.0, 26.0),
                (67.1, 26.7),
                (73.2, 28.8),
                (78.3, 31.9),
                (82.0, 36.0),
                (74.0, 44.0),
                (74.3, 48.7),
                (74.0, 53.3),
                (73.2, 57.7),
                (71.9, 61.9),
                (70.0, 65.8),
                (67.7, 69.3),
                (64.9, 72.6),
                (61.6, 75.5),
                (58.0, 78.0),
                (54.0, 80.0),
            ]),
            RUFOUS,
        ),
        (Shape::Poly(&[(82.0, 36.0), (90.0, 44.0), (76.0, 46.0)]), BEAK),
        (Shape::Circle((64.0, 38.0), 4.5), TILE),
        (
            Shape::Poly(&[
                (58.0, 46.0),
                (57.7, 50.8),
                (56.7, 55.3),
                (55.2, 59.5),
                (53.3, 63.3),
                (50.8, 66.8),
                (48.0, 70.0),
                (54.0, 70.0),
                (57.7, 66.7),
                (60.6, 62.9),
                (62.8, 58.8),
                (64.1, 54.4),
                (64.5, 50.1),
                (64.0, 46.0),
            ]),
            TILE,
        ),
    ];
    let unit = 100.0 / n as f32; // one pixel in grid units
                                 // Small sizes: beak, eye and stripe grow by about half a pixel so they don't fade into the head.
    let detail = if n <= 24 { unit * 0.6 } else { 0.0 };
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let p = ((x as f32 + 0.5) * unit, (y as f32 + 0.5) * unit);
            // Rounded square, corner radius 22.
            let (qx, qy) = ((p.0 - 50.0).abs() - 28.0, (p.1 - 50.0).abs() - 28.0);
            let square = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - 22.0;
            let (mut c, mut bird) = (TILE, 0.0f32);
            for (i, (shape, color)) in layers.iter().enumerate() {
                let a = (0.5 - (dist(shape, p) - if i > 0 { detail } else { 0.0 }) / unit).clamp(0.0, 1.0);
                if *color == TILE {
                    bird *= 1.0 - a; // eye and stripe: holes in the bird
                } else {
                    bird += (1.0 - bird) * a;
                }
                if tile || *color != TILE {
                    c = [0, 1, 2].map(|i| c[i] + (color[i] - c[i]) * a);
                }
            }
            let alpha = if tile { (0.5 - square / unit).clamp(0.0, 1.0) } else { bird };
            rgba.extend([c[0] as u8, c[1] as u8, c[2] as u8, (alpha * 255.0) as u8]);
        }
    }
    rgba
}
