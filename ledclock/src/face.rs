// SPDX-License-Identifier: GPL-3.0-or-later
//! The picture behind the digits: a metal frame, lit from the top left,
//! around a dark display panel that lies a little below it.
//!
//! Every number here was measured on a design of 274x90 pixels and is in
//! the pixels of that design: `x` runs from 0 to `WIDTH`, `y` from 0 to
//! `HEIGHT`, and pixel `i` of the design covers `i..i + 1`. The picture is
//! computed, not stored, so that it is as sharp at 2 times the size as at 1.

pub const WIDTH: f64 = 274.0;
pub const HEIGHT: f64 = 90.0;

/// The panel, inside the frame: `PANEL_LEFT..PANEL_RIGHT` by
/// `PANEL_TOP..PANEL_BOTTOM`.
pub const PANEL_LEFT: f64 = 15.0;
pub const PANEL_RIGHT: f64 = 258.0;
const PANEL_TOP: f64 = 13.0;
const PANEL_BOTTOM: f64 = 77.0;
/// The frame ends in a lip above and below the panel.
const LIP_TOP: f64 = 10.0;
const LIP_BOTTOM: f64 = 81.0;

/// The frame's grey from its top row to its bottom row: a bright edge, then
/// darker all the way down.
const FRAME: [(f64, f64); 22] = [
    (0.0, 143.0),
    (1.0, 177.0),
    (2.0, 228.0),
    (3.0, 224.0),
    (4.0, 219.0),
    (5.0, 213.0),
    (6.0, 205.0),
    (7.0, 198.0),
    (8.0, 192.0),
    (9.0, 190.0),
    (13.0, 184.0),
    (45.0, 137.0),
    (77.0, 90.0),
    (81.0, 83.0),
    (82.0, 76.0),
    (83.0, 64.0),
    (84.0, 51.0),
    (85.0, 39.0),
    (86.0, 27.0),
    (87.0, 19.0),
    (88.0, 8.0),
    (89.0, 0.0),
];
/// The dark line down the frame's left edge.
const LEFT_EDGE: [(f64, f64); 7] =
    [(0.0, 151.0), (1.0, 145.0), (5.0, 96.0), (11.0, 62.0), (77.0, 62.0), (84.0, 23.0), (89.0, 0.0)];
/// The light the frame catches beside that line, by column.
const LEFT_LIGHT: [(f64, f64); 8] =
    [(2.0, 28.0), (3.0, 26.0), (4.0, 23.0), (5.0, 18.0), (6.0, 12.0), (7.0, 5.0), (8.0, 1.0), (9.0, 0.0)];
/// How much of the grey is left towards the right edge, by distance from it.
const RIGHT_SHADE: [(f64, f64); 10] = [
    (0.0, 0.0),
    (1.0, 0.19),
    (2.0, 0.5),
    (3.0, 0.6),
    (4.0, 0.68),
    (5.0, 0.78),
    (6.0, 0.88),
    (7.0, 0.96),
    (8.0, 0.99),
    (9.0, 1.0),
];
/// Near the top right corner the frame turns dark later: how many pixels
/// later, by row.
const RIGHT_CORNER: [(f64, f64); 9] =
    [(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 3.5), (4.0, 3.0), (5.0, 2.0), (6.0, 1.0), (7.0, 0.5), (8.0, 0.0)];
const TOP_LIP: [(f64, f64); 2] = [(10.0, 164.0), (11.0, 152.0)];
const BOTTOM_LIP: [(f64, f64); 3] = [(78.0, 152.0), (79.0, 152.0), (80.0, 120.0)];
/// The lines between the lip and the panel.
const TOP_LINE: f64 = 102.0;
const BOTTOM_LINE: f64 = 106.0;
/// The column left of the panel, down to the end of the bottom lip: where
/// the top line would be, the dark line beside the panel, and a dimmer lip.
const LEFT_LINE: [(f64, f64); 8] =
    [(12.0, 129.0), (13.0, 47.0), (45.0, 43.0), (76.0, 32.0), (77.0, 78.0), (78.0, 103.0), (79.0, 102.0), (80.0, 94.0)];
/// The column right of the panel, from the top lip to the bottom one: a
/// lighter line beside the panel, and dimmer lips.
const RIGHT_LINE: [(f64, f64); 16] = [
    (10.0, 182.0),
    (11.0, 178.0),
    (12.0, 162.0),
    (13.0, 137.0),
    (21.0, 139.0),
    (29.0, 135.0),
    (37.0, 127.0),
    (45.0, 119.0),
    (53.0, 112.0),
    (61.0, 104.0),
    (69.0, 97.0),
    (76.0, 90.0),
    (77.0, 99.0),
    (78.0, 104.0),
    (79.0, 103.0),
    (80.0, 94.0),
];

/// The panel's green at row 27, where the frame's shadow ends, and how much
/// darker each row below it is.
const PANEL_GREEN: f64 = 72.0;
const PANEL_ROW: f64 = 27.0;
const PANEL_SLOPE: f64 = 0.5;
/// Red and blue of the panel, as parts of its green.
const PANEL_RED: f64 = 0.84;
const PANEL_BLUE: f64 = 0.905;
/// The shadow the frame casts on the panel, by distance from the panel's
/// top, left and right edges: how much of the colour is left.
const TOP_SHADOW: [(f64, f64); 15] = [
    (0.0, 0.28),
    (1.0, 0.31),
    (2.0, 0.35),
    (3.0, 0.39),
    (4.0, 0.46),
    (5.0, 0.54),
    (6.0, 0.62),
    (7.0, 0.70),
    (8.0, 0.79),
    (9.0, 0.85),
    (10.0, 0.91),
    (11.0, 0.95),
    (12.0, 0.97),
    (13.0, 0.99),
    (14.0, 1.0),
];
const LEFT_SHADOW: [(f64, f64); 11] = [
    (0.0, 0.41),
    (1.0, 0.49),
    (2.0, 0.57),
    (3.0, 0.65),
    (4.0, 0.73),
    (5.0, 0.81),
    (6.0, 0.87),
    (7.0, 0.92),
    (8.0, 0.95),
    (9.0, 0.98),
    (10.0, 1.0),
];
const RIGHT_SHADOW: [(f64, f64); 4] = [(0.0, 0.93), (1.0, 0.96), (2.0, 0.98), (3.0, 1.0)];

/// The value of `table` at `at`: its points joined by straight lines, the
/// first and the last value outside them.
fn along(table: &[(f64, f64)], at: f64) -> f64 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if at <= first.0 {
        return first.1;
    }
    for pair in table.windows(2) {
        let ((a, va), (b, vb)) = (pair[0], pair[1]);
        if at <= b {
            return va + (vb - va) * (at - a) / (b - a);
        }
    }
    last.1
}

fn grey(value: f64) -> [f64; 3] {
    [value, value, value]
}

/// The frame at column `col` and row `row`, counted from pixel centres.
fn frame(col: f64, row: f64) -> f64 {
    let face = along(&FRAME, row);
    // The light adds less where the frame is bright already.
    let lit = |col: f64| face + along(&LEFT_LIGHT, col) * ((235.0 - face) / 100.0).clamp(0.0, 1.0);
    let edge = along(&LEFT_EDGE, row);
    // The second column is between the dark line and the lit frame.
    let second = 0.55 * edge + 0.45 * lit(2.0);
    let value = if col < 1.0 {
        edge + (second - edge) * col.max(0.0)
    } else if col < 2.0 {
        second + (lit(2.0) - second) * (col - 1.0)
    } else {
        lit(col)
    };
    let from_right = WIDTH - 1.0 - col;
    // The two columns at the very edge are dark up to the corner.
    let later = along(&RIGHT_CORNER, row) * (from_right - 1.0).clamp(0.0, 1.0);
    value * along(&RIGHT_SHADE, from_right + later)
}

/// The panel at column `col` and row `row`, counted from pixel centres.
fn panel(col: f64, row: f64) -> [f64; 3] {
    let green = PANEL_GREEN - PANEL_SLOPE * (row - PANEL_ROW);
    // Where the top and the left shadow meet, the darker one shows.
    let shadow = along(&TOP_SHADOW, row - PANEL_TOP).min(along(&LEFT_SHADOW, col - PANEL_LEFT))
        * along(&RIGHT_SHADOW, PANEL_RIGHT - 1.0 - col);
    let green = green * shadow;
    [green * PANEL_RED, green, green * PANEL_BLUE]
}

/// The colour at the point `x`, `y` of the design: red, green and blue from
/// 0 to 255.
pub fn color(x: f64, y: f64) -> [f64; 3] {
    // The tables give the colour at the centre of each design pixel.
    let (col, row) = (x - 0.5, y - 0.5);
    let between_lips = (LIP_TOP..LIP_BOTTOM).contains(&y);
    if (PANEL_LEFT - 1.0..PANEL_LEFT).contains(&x) && (PANEL_TOP - 1.0..LIP_BOTTOM).contains(&y) {
        grey(along(&LEFT_LINE, row))
    } else if (PANEL_RIGHT..PANEL_RIGHT + 1.0).contains(&x) && between_lips {
        grey(along(&RIGHT_LINE, row))
    } else if !(PANEL_LEFT..PANEL_RIGHT).contains(&x) || !between_lips {
        grey(frame(col, row))
    } else if y < PANEL_TOP - 1.0 {
        grey(along(&TOP_LIP, row))
    } else if y >= PANEL_BOTTOM + 1.0 {
        grey(along(&BOTTOM_LIP, row))
    } else if y < PANEL_TOP {
        grey(TOP_LINE)
    } else if y >= PANEL_BOTTOM {
        grey(BOTTOM_LINE)
    } else {
        panel(col, row)
    }
}

/// The picture at `width` x `height` pixels, row by row: the colour of each
/// pixel is the mean of `samples` x `samples` points inside it.
pub fn render(width: usize, height: usize, samples: usize) -> Vec<[u8; 3]> {
    let (sx, sy) = (WIDTH / width as f64, HEIGHT / height as f64);
    let mut out = Vec::with_capacity(width * height);
    for py in 0..height {
        for px in 0..width {
            let mut sum = [0.0; 3];
            for j in 0..samples {
                for i in 0..samples {
                    let x = (px as f64 + (i as f64 + 0.5) / samples as f64) * sx;
                    let y = (py as f64 + (j as f64 + 0.5) / samples as f64) * sy;
                    let c = color(x, y);
                    for k in 0..3 {
                        sum[k] += c[k];
                    }
                }
            }
            let n = (samples * samples) as f64;
            out.push(sum.map(|v| (v / n).round().clamp(0.0, 255.0) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colour of pixel `x`, `y` of the design at its own size.
    fn design(x: usize, y: usize) -> [u8; 3] {
        color(x as f64 + 0.5, y as f64 + 0.5).map(|v| v.round() as u8)
    }

    fn near(got: [u8; 3], want: u32, tolerance: i32) -> bool {
        let want = [(want >> 16) as u8, (want >> 8) as u8, want as u8];
        (0..3).all(|k| (got[k] as i32 - want[k] as i32).abs() <= tolerance)
    }

    #[test]
    fn tables() {
        let t = [(1.0, 10.0), (3.0, 30.0), (4.0, 0.0)];
        assert_eq!(along(&t, 0.0), 10.0);
        assert_eq!(along(&t, 2.0), 20.0);
        assert_eq!(along(&t, 3.5), 15.0);
        assert_eq!(along(&t, 9.0), 0.0);
    }

    /// Pixels of the design the picture was measured on.
    #[test]
    fn design_pixels() {
        let samples: [(usize, usize, u32, i32); 25] = [
            // the frame: top, left, bottom, right, and its corners
            (136, 2, 0xe4e4e4, 1),
            (136, 9, 0xbebebe, 1),
            (9, 45, 0x898989, 1),
            (2, 45, 0xa5a5a5, 2),
            (0, 45, 0x3e3e3e, 1),
            (136, 84, 0x333333, 1),
            (136, 89, 0x000000, 0),
            (266, 45, 0x838383, 2),
            (272, 45, 0x191919, 2),
            (0, 0, 0x979797, 1),
            (3, 84, 0x4b4b4b, 4),
            (271, 2, 0xcbcbcb, 6),
            (270, 5, 0xa6a6a6, 6),
            (14, 11, 0xb3b3b3, 8),
            // the lip and the lines around the panel
            (136, 11, 0x989898, 1),
            (136, 78, 0x989898, 1),
            (136, 12, 0x666867, 2),
            (14, 45, 0x282b29, 3),
            (14, 78, 0x676767, 2),
            (258, 45, 0x757776, 2),
            (258, 79, 0x676767, 2),
            // the panel: its middle, the top shadow, the left shadow
            (136, 45, 0x353f39, 1),
            (70, 76, 0x272f2a, 2),
            (136, 13, 0x121613, 3),
            (15, 45, 0x161a18, 2),
        ];
        for (x, y, want, tolerance) in samples {
            assert!(near(design(x, y), want, tolerance), "pixel {x},{y}: {:?}, wanted {want:06x}", design(x, y));
        }
    }

    #[test]
    fn rendered_sizes() {
        let one = render(274, 90, 1);
        assert_eq!(one.len(), 274 * 90);
        assert_eq!(one[45 * 274 + 136], design(136, 45));
        // Twice the size: the middle of the panel is the same colour.
        let two = render(548, 180, 1);
        assert!(near(two[90 * 548 + 272], 0x353f39, 2));
        assert_eq!(render(137, 45, 3).len(), 137 * 45);
    }
}
