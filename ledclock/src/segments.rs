// SPDX-License-Identifier: GPL-3.0-or-later
//! The digits of the clock, as an LED display has them: four digits of
//! seven segments each, and the two dots of a colon between the hours and
//! the minutes. Every segment is there all the time, as on a display that
//! shows 88:88; the time is in which of them are lit.
//!
//! Lengths are in pixels of the design (src/face.rs), from the top left
//! corner of the display, which leans to the right.

use std::f64::consts::FRAC_1_SQRT_2;

/// The height of a digit.
pub const HEIGHT: f64 = 39.0;
/// The width of a digit, and of a segment across its length.
const DIGIT: f64 = 22.0;
const THICK: f64 = 5.0;
/// From the middle of a segment to its tip. The lines the segments are on
/// meet at the corners of a digit; every tip stops short of the corner.
const REACH: f64 = (DIGIT - THICK) / 2.0 - 0.7;
/// Where each of the four digits starts: 8 pixels between two digits, 14
/// for the colon.
const DIGITS: [f64; 4] = [0.0, 30.0, 66.0, 96.0];
/// The width of the display.
pub const WIDTH: f64 = DIGITS[3] + DIGIT;
/// The two dots of the colon: the middle of each and its radius.
const DOTS: [(f64, f64); 2] = [(59.0, 12.5), (59.0, 26.5)];
const DOT: f64 = 2.75;
/// The lean: how far to the right a row is for every pixel it is above the
/// middle of the display.
const SLANT: f64 = 0.08;
/// How far the top of the display is to the right of where it would stand
/// upright, and the bottom to the left.
pub const LEAN: f64 = SLANT * HEIGHT / 2.0;

/// The seven segments of a digit, a to g: the middle of each, from the top
/// left corner of the digit, and whether it lies across.
///
/// ```text
///  -a-
/// f   b
///  -g-
/// e   c
///  -d-
/// ```
const SEGMENTS: [(f64, f64, bool); 7] = [
    (11.0, 2.5, true),
    (19.5, 11.0, false),
    (19.5, 28.0, false),
    (11.0, 36.5, true),
    (2.5, 28.0, false),
    (2.5, 11.0, false),
    (11.0, 19.5, true),
];

/// The segments that are lit for each digit from 0 to 9: bit 0 is a, bit 6
/// is g.
const LIT: [u8; 10] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f];

/// How much of the pixel around the point `x`, `y` is in a segment that is
/// lit, and how much in one that is not, each from 0 to 1, on a display
/// that shows `digits`. `pixel` is the width of a pixel of the screen.
pub fn cover(x: f64, y: f64, pixel: f64, digits: [u8; 4]) -> (f64, f64) {
    // As on a display that stands upright.
    let x = x - SLANT * (HEIGHT / 2.0 - y);
    // From the distance to the edge of a shape, outside of it.
    let inside = |distance: f64| (0.5 - distance / pixel).clamp(0.0, 1.0);

    // The colon is lit all the time.
    let mut lit = DOTS.iter().map(|&(cx, cy)| inside((x - cx).hypot(y - cy) - DOT)).fold(0.0, f64::max);
    let mut unlit: f64 = 0.0;
    for (&left, &digit) in DIGITS.iter().zip(&digits) {
        if x < left - pixel || x > left + DIGIT + pixel {
            continue;
        }
        for (bit, &(cx, cy, across)) in SEGMENTS.iter().enumerate() {
            let (dx, dy) = ((x - left - cx).abs(), (y - cy).abs());
            let (along, off) = if across { (dx, dy) } else { (dy, dx) };
            // A bar with a point at each end.
            let part = inside((off - THICK / 2.0).max((along + off - REACH) * FRAC_1_SQRT_2));
            if LIT[digit as usize % 10] >> bit & 1 == 1 {
                lit = lit.max(part);
            } else {
                unlit = unlit.max(part);
            }
        }
    }
    (lit, unlit)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point at `x`, `y` of the display standing upright.
    fn at(x: f64, y: f64, digits: [u8; 4]) -> (f64, f64) {
        cover(x + SLANT * (HEIGHT / 2.0 - y), y, 1.0, digits)
    }

    #[test]
    fn lit_segments() {
        let count: Vec<u32> = LIT.iter().map(|s| s.count_ones()).collect();
        assert_eq!(count, [6, 2, 5, 5, 4, 5, 6, 3, 7, 6]);
        // 1 is the two segments on the right, 7 has the top one too.
        assert_eq!(LIT[1], 1 << 1 | 1 << 2);
        assert_eq!(LIT[7], LIT[1] | 1);
        // 8 without the middle is 0; without the lower left, 9.
        assert_eq!(LIT[8] & !(1 << 6), LIT[0]);
        assert_eq!(LIT[8] & !(1 << 4), LIT[9]);
    }

    #[test]
    fn display() {
        assert_eq!(WIDTH, 118.0);
        // The middle of the first digit: on g, lit for 8 and not for 0.
        assert_eq!(at(11.0, 19.5, [8, 8, 8, 8]), (1.0, 0.0));
        assert_eq!(at(11.0, 19.5, [0, 8, 8, 8]), (0.0, 1.0));
        // The second digit showing 1: b is lit, f is not.
        assert_eq!(at(30.0 + 19.5, 11.0, [0, 1, 0, 0]), (1.0, 0.0));
        assert_eq!(at(30.0 + 2.5, 11.0, [0, 1, 0, 0]), (0.0, 1.0));
        // Inside a digit and between two digits there is nothing.
        assert_eq!(at(11.0, 11.0, [8, 8, 8, 8]), (0.0, 0.0));
        assert_eq!(at(26.0, 19.5, [8, 8, 8, 8]), (0.0, 0.0));
        // The corner of a digit is between the tips of a and f.
        let (lit, unlit) = at(2.5, 2.5, [8, 8, 8, 8]);
        assert!(lit < 0.05 && unlit == 0.0, "{lit} {unlit}");
        // The dots of the colon.
        assert_eq!(at(59.0, 12.5, [0, 0, 0, 0]), (1.0, 0.0));
        assert_eq!(at(59.0, 26.5, [0, 0, 0, 0]), (1.0, 0.0));
        assert_eq!(at(59.0, 19.5, [0, 0, 0, 0]), (0.0, 0.0));
        // The edge of a segment covers half of the pixel on it.
        assert_eq!(at(11.0, 5.0, [8, 8, 8, 8]), (0.5, 0.0));
        // The top leans to the right, the bottom to the left.
        assert_eq!(cover(11.0 + LEAN, 0.5, 1.0, [8, 8, 8, 8]).0, 1.0);
        assert_eq!(cover(11.0 - LEAN, 38.5, 1.0, [8, 8, 8, 8]).0, 1.0);
    }
}
