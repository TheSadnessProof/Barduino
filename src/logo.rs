//! Megobari AI's Borjgali: seven spiral blades turning around one centre. It is
//! the app's mark, drawn from the outline in `assets/borjgali.svg` — as shapes
//! on screen, and rasterised for the window icon.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Vec2};

/// The blades, spaced evenly around the centre.
pub const BLADES: usize = 7;

/// The burgundy the mark is drawn in.
pub const COLOUR: Color32 = Color32::from_rgb(0x80, 0x00, 0x20);

/// One blade's leading edge, from the centre out to the hooked tip, and its
/// trailing edge running the same way so the two pair up point for point. Both
/// are centred on the origin, with the SVG's 100×100 box scaled to -1…1.
///
/// Regenerate these from the SVG rather than editing them by hand.
const LEADING: [[f32; 2]; 61] = [
    [0.0, 0.0], [0.0234, -0.0242], [0.0306, -0.0418], [0.0366, -0.0574], [0.0424, -0.072], [0.0484, -0.0858],
    [0.0546, -0.0988], [0.0612, -0.1112], [0.0684, -0.1232], [0.076, -0.1348], [0.0842, -0.1458], [0.093, -0.1564],
    [0.1024, -0.1666], [0.1124, -0.1764], [0.1232, -0.1856], [0.1344, -0.1946], [0.1466, -0.203], [0.1592, -0.2108],
    [0.1728, -0.218], [0.1868, -0.2248], [0.2016, -0.2308], [0.217, -0.236], [0.2332, -0.2406], [0.2498, -0.2446],
    [0.2672, -0.2476], [0.285, -0.2496], [0.3036, -0.2508], [0.3224, -0.2512], [0.3418, -0.2504], [0.3616, -0.2486],
    [0.3818, -0.2458], [0.4022, -0.2418], [0.4228, -0.2366], [0.4438, -0.2304], [0.4648, -0.223], [0.486, -0.2142],
    [0.5072, -0.2044], [0.5284, -0.1932], [0.5494, -0.1808], [0.5702, -0.167], [0.5908, -0.152], [0.6112, -0.1358],
    [0.6312, -0.1182], [0.6508, -0.0994], [0.6698, -0.0792], [0.6882, -0.0578], [0.706, -0.0352], [0.723, -0.0112],
    [0.7394, 0.0138], [0.7548, 0.04], [0.7694, 0.0676], [0.783, 0.096], [0.7954, 0.1258], [0.807, 0.1564],
    [0.8172, 0.1882], [0.8262, 0.2208], [0.8342, 0.2544], [0.8406, 0.289], [0.8458, 0.3244], [0.8494, 0.3604],
    [0.852, 0.3972],
];

const TRAILING: [[f32; 2]; 60] = [
    [0.0, 0.0], [-0.0294, -0.019], [-0.039, -0.0402], [-0.0444, -0.0618], [-0.047, -0.0836], [-0.047, -0.1054],
    [-0.0448, -0.127], [-0.0404, -0.1484], [-0.0342, -0.1692], [-0.026, -0.1896], [-0.0162, -0.2094],
    [-0.0048, -0.2284], [0.0084, -0.2466], [0.0228, -0.2638], [0.0386, -0.2798], [0.0558, -0.2948],
    [0.0742, -0.3088], [0.0936, -0.3214], [0.114, -0.3328], [0.1354, -0.343], [0.1576, -0.3516], [0.1806, -0.359],
    [0.2042, -0.3648], [0.2282, -0.3694], [0.253, -0.3724], [0.278, -0.374], [0.3034, -0.374], [0.329, -0.3724],
    [0.3548, -0.3696], [0.3806, -0.365], [0.4066, -0.3592], [0.4324, -0.3516], [0.458, -0.3428], [0.4834, -0.3322],
    [0.5086, -0.3204], [0.5334, -0.307], [0.5578, -0.2922], [0.5816, -0.276], [0.6048, -0.2584], [0.6274, -0.2396],
    [0.6494, -0.2192], [0.6706, -0.1978], [0.6908, -0.1748], [0.7102, -0.1508], [0.7286, -0.1256], [0.746, -0.0992],
    [0.7624, -0.0718], [0.7776, -0.0434], [0.7918, -0.0138], [0.8046, 0.0166], [0.8162, 0.048], [0.8264, 0.0802],
    [0.8352, 0.1132], [0.8426, 0.1468], [0.8486, 0.1812], [0.8532, 0.216], [0.8562, 0.2516], [0.8576, 0.2874],
    [0.8574, 0.3238], [0.8556, 0.3604],
];

/// Draws the mark centred in `rect`, as large as fits.
pub fn paint(painter: &egui::Painter, rect: Rect, colour: Color32) {
    let radius = rect.width().min(rect.height()) / 2.0;
    let centre = rect.center();
    for blade in 0..BLADES {
        let (sin, cos) = turn(blade).sin_cos();
        let place = |[x, y]: [f32; 2]| centre + Vec2::new(x * cos - y * sin, x * sin + y * cos) * radius;
        painter.add(blade_mesh(&place, colour));
    }
}

/// How far round the mark this blade sits.
fn turn(blade: usize) -> f32 {
    blade as f32 * std::f32::consts::TAU / BLADES as f32
}

/// One blade as a strip of triangles between its two edges. egui fills a closed
/// path with a fan from its first point, which only comes out right for a shape
/// that is convex from there — a spiral blade is nowhere near, so the triangles
/// are built by hand instead.
fn blade_mesh(place: &impl Fn([f32; 2]) -> Pos2, colour: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    let pairs = TRAILING.len();
    mesh.reserve_vertices(2 * pairs + 1);
    mesh.reserve_triangles(2 * pairs - 1);
    for i in 0..pairs {
        mesh.colored_vertex(place(LEADING[i]), colour);
        mesh.colored_vertex(place(TRAILING[i]), colour);
    }
    // The tip, the one point where the two edges meet.
    mesh.colored_vertex(place(LEADING[pairs]), colour);

    for i in 0..pairs - 1 {
        let (lead, trail) = (2 * i as u32, 2 * i as u32 + 1);
        mesh.add_triangle(lead, trail, lead + 2);
        mesh.add_triangle(trail, trail + 2, lead + 2);
    }
    let tip = 2 * pairs as u32;
    mesh.add_triangle(tip - 2, tip - 1, tip);
    mesh
}

/// One blade's closed outline: out along the leading edge, back along the trailing one.
fn outline() -> Vec<[f32; 2]> {
    LEADING.iter().chain(TRAILING.iter().rev()).copied().collect()
}

/// Rasterises the mark for the window icon, which needs pixels rather than shapes.
pub fn window_icon(size: u32) -> egui::IconData {
    /// Each icon pixel is rasterised this many times across and down and then
    /// averaged, which is what keeps the spiral edges from coming out jagged.
    const SAMPLES: u32 = 3;

    let side = size * SAMPLES;
    let mut covered = vec![false; (side * side) as usize];
    let outline = outline();
    for blade in 0..BLADES {
        let (sin, cos) = turn(blade).sin_cos();
        let turned: Vec<[f32; 2]> =
            outline.iter().map(|[x, y]| [x * cos - y * sin, x * sin + y * cos]).collect();
        fill(&turned, side, &mut covered);
    }

    let [red, green, blue, _] = COLOUR.to_array();
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let (row, column) = (y * SAMPLES + sy, x * SAMPLES + sx);
                    hits += u32::from(covered[(row * side + column) as usize]);
                }
            }
            // Unmultiplied alpha, which is what IconData wants.
            rgba.extend_from_slice(&[red, green, blue, (hits * 255 / (SAMPLES * SAMPLES)) as u8]);
        }
    }
    egui::IconData { rgba, width: size, height: size }
}

/// Marks the pixels one blade covers on a `side`×`side` grid over -1…1, by
/// working out where each row crosses the outline. Testing every pixel against
/// every edge instead would be millions of tests for one small icon.
fn fill(outline: &[[f32; 2]], side: u32, covered: &mut [bool]) {
    let column_of = |x: f32| (x + 1.0) / 2.0 * side as f32 - 0.5;
    let mut crossings: Vec<f32> = Vec::new();
    for row in 0..side {
        // The middle of this row, back in the mark's own -1…1 space.
        let y = (row as f32 + 0.5) / side as f32 * 2.0 - 1.0;
        crossings.clear();
        for i in 0..outline.len() {
            let [x1, y1] = outline[i];
            let [x2, y2] = outline[(i + 1) % outline.len()];
            if (y1 > y) != (y2 > y) {
                crossings.push(x1 + (y - y1) / (y2 - y1) * (x2 - x1));
            }
        }
        crossings.sort_by(f32::total_cmp);
        // Between the first and second crossing is inside, the second and third
        // outside, and so on, so the pairs are the spans to fill.
        for span in crossings.as_chunks::<2>().0 {
            let from = column_of(span[0]).ceil().clamp(0.0, side as f32) as u32;
            let until = column_of(span[1]).ceil().clamp(0.0, side as f32) as u32;
            for column in from..until {
                covered[(row * side + column) as usize] = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_edges_of_a_blade_pair_up_from_centre_to_tip() {
        // Both edges start at the centre and run outwards, which is what lets a
        // blade be drawn as a strip of triangles between them.
        assert_eq!(LEADING[0], [0.0, 0.0]);
        assert_eq!(TRAILING[0], [0.0, 0.0]);
        assert_eq!(LEADING.len(), TRAILING.len() + 1, "the leading edge also carries the tip");

        let radius = |[x, y]: [f32; 2]| (x * x + y * y).sqrt();
        let reach = LEADING.iter().copied().map(radius).fold(0.0_f32, f32::max);
        assert!((0.9..1.0).contains(&reach), "the tip reaches {reach:.3} of the way out, leaving a margin");
        assert_eq!(outline().len(), LEADING.len() + TRAILING.len(), "the outline is both edges, once each");
    }

    #[test]
    fn the_window_icon_draws_the_whole_mark() {
        let icon = window_icon(64);
        assert_eq!((icon.width, icon.height), (64, 64));
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);

        // Only alpha shapes the mark; the burgundy is everywhere underneath.
        let [red, green, blue, _] = COLOUR.to_array();
        assert!(icon.rgba.as_chunks::<4>().0.iter().all(|pixel| pixel[..3] == [red, green, blue]));

        let alpha = |x: u32, y: u32| icon.rgba[((y * 64 + x) * 4 + 3) as usize];
        // Seven blades cover a bit over a fifth of the square they turn in. This
        // was worked out from the outline separately, so a wildly different figure
        // means the scanlines are filling the wrong spans.
        let covered: f32 = (0..64u32)
            .flat_map(|y| (0..64u32).map(move |x| (x, y)))
            .map(|(x, y)| f32::from(alpha(x, y)) / 255.0)
            .sum::<f32>()
            / (64.0 * 64.0);
        assert!((0.20..0.23).contains(&covered), "the blades cover {covered:.4} of the icon");

        // All seven blades meet at the centre, and none of them reaches a corner.
        assert_eq!(alpha(32, 32), 255, "the centre is solid");
        for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
            assert_eq!(alpha(x, y), 0, "the corner at {x},{y} is clear");
        }
    }
}
