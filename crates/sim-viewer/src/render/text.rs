//! Bitmap font (5x7 cells with two descender rows, upper and lower case) drawn as instances.

use super::gpu::Instance;

/// Glyph advance and line height, in font pixels.
pub(super) const ADVANCE: f32 = 6.0;
pub(super) const LINE: f32 = 10.0;

/// Width of `text` in screen pixels at font pixel size `pixel` (no trailing gap).
pub(super) fn text_width(text: &str, pixel: f32) -> f32 {
    let count = text.chars().count() as f32;
    (count * ADVANCE - 1.0).max(0.0) * pixel
}

/// Draws `text` with its top-left at (`x`, `y`) and returns the x just past it.
pub(super) fn push_text(
    instances: &mut Vec<Instance>,
    text: &str,
    x: f32,
    y: f32,
    pixel: f32,
    color: u32,
) -> f32 {
    let mut glyph_x = x;
    for character in text.chars() {
        for (row_index, row) in glyph_rows(character).into_iter().enumerate() {
            let mut column = 0;
            while column < 5 {
                if row & (1 << (4 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 5 && row & (1 << (4 - column)) != 0 {
                    column += 1;
                }
                instances.push(Instance::new(
                    glyph_x + start as f32 * pixel,
                    y + row_index as f32 * pixel,
                    (column - start) as f32 * pixel,
                    pixel,
                    color,
                ));
            }
        }
        glyph_x += ADVANCE * pixel;
    }
    glyph_x
}

/// Splits `text` into lines of at most `width` characters, breaking at spaces
/// where possible.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let needed = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
        if needed > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Rows top to bottom, five bits each (bit 4 is the left column). Rows 0-6
/// hold capitals and x-height letters (rows 2-6); rows 7-8 are descenders.
const fn glyph_rows(character: char) -> [u8; 9] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17, 0, 0],
        'B' => [30, 17, 17, 30, 17, 17, 30, 0, 0],
        'C' => [14, 17, 16, 16, 16, 17, 14, 0, 0],
        'D' => [30, 17, 17, 17, 17, 17, 30, 0, 0],
        'E' => [31, 16, 16, 30, 16, 16, 31, 0, 0],
        'F' => [31, 16, 16, 30, 16, 16, 16, 0, 0],
        'G' => [14, 17, 16, 23, 17, 17, 15, 0, 0],
        'H' => [17, 17, 17, 31, 17, 17, 17, 0, 0],
        'I' => [14, 4, 4, 4, 4, 4, 14, 0, 0],
        'J' => [7, 2, 2, 2, 18, 18, 12, 0, 0],
        'K' => [17, 18, 20, 24, 20, 18, 17, 0, 0],
        'L' => [16, 16, 16, 16, 16, 16, 31, 0, 0],
        'M' => [17, 27, 21, 21, 17, 17, 17, 0, 0],
        'N' => [17, 25, 21, 19, 17, 17, 17, 0, 0],
        'O' => [14, 17, 17, 17, 17, 17, 14, 0, 0],
        'P' => [30, 17, 17, 30, 16, 16, 16, 0, 0],
        'Q' => [14, 17, 17, 17, 21, 18, 13, 0, 0],
        'R' => [30, 17, 17, 30, 20, 18, 17, 0, 0],
        'S' => [15, 16, 16, 14, 1, 1, 30, 0, 0],
        'T' => [31, 4, 4, 4, 4, 4, 4, 0, 0],
        'U' => [17, 17, 17, 17, 17, 17, 14, 0, 0],
        'V' => [17, 17, 17, 17, 17, 10, 4, 0, 0],
        'W' => [17, 17, 17, 21, 21, 21, 10, 0, 0],
        'X' => [17, 17, 10, 4, 10, 17, 17, 0, 0],
        'Y' => [17, 17, 10, 4, 4, 4, 4, 0, 0],
        'Z' => [31, 1, 2, 4, 8, 16, 31, 0, 0],
        'a' => [0, 0, 14, 1, 15, 17, 15, 0, 0],
        'b' => [16, 16, 22, 25, 17, 17, 30, 0, 0],
        'c' => [0, 0, 14, 16, 16, 17, 14, 0, 0],
        'd' => [1, 1, 13, 19, 17, 17, 15, 0, 0],
        'e' => [0, 0, 14, 17, 31, 16, 14, 0, 0],
        'f' => [6, 9, 8, 28, 8, 8, 8, 0, 0],
        'g' => [0, 0, 15, 17, 17, 17, 15, 1, 14],
        'h' => [16, 16, 22, 25, 17, 17, 17, 0, 0],
        'i' => [4, 0, 12, 4, 4, 4, 14, 0, 0],
        'j' => [2, 0, 6, 2, 2, 2, 2, 18, 12],
        'k' => [16, 16, 18, 20, 24, 20, 18, 0, 0],
        'l' => [12, 4, 4, 4, 4, 4, 14, 0, 0],
        'm' => [0, 0, 26, 21, 21, 17, 17, 0, 0],
        'n' => [0, 0, 22, 25, 17, 17, 17, 0, 0],
        'o' => [0, 0, 14, 17, 17, 17, 14, 0, 0],
        'p' => [0, 0, 30, 17, 17, 17, 30, 16, 16],
        'q' => [0, 0, 15, 17, 17, 17, 15, 1, 1],
        'r' => [0, 0, 22, 25, 16, 16, 16, 0, 0],
        's' => [0, 0, 15, 16, 14, 1, 30, 0, 0],
        't' => [8, 8, 28, 8, 8, 9, 6, 0, 0],
        'u' => [0, 0, 17, 17, 17, 19, 13, 0, 0],
        'v' => [0, 0, 17, 17, 17, 10, 4, 0, 0],
        'w' => [0, 0, 17, 17, 21, 21, 10, 0, 0],
        'x' => [0, 0, 17, 10, 4, 10, 17, 0, 0],
        'y' => [0, 0, 17, 17, 17, 17, 15, 1, 14],
        'z' => [0, 0, 31, 2, 4, 8, 31, 0, 0],
        '0' => [14, 17, 19, 21, 25, 17, 14, 0, 0],
        '1' => [4, 12, 4, 4, 4, 4, 14, 0, 0],
        '2' => [14, 17, 1, 2, 4, 8, 31, 0, 0],
        '3' => [30, 1, 1, 14, 1, 1, 30, 0, 0],
        '4' => [2, 6, 10, 18, 31, 2, 2, 0, 0],
        '5' => [31, 16, 16, 30, 1, 1, 30, 0, 0],
        '6' => [14, 16, 16, 30, 17, 17, 14, 0, 0],
        '7' => [31, 1, 2, 4, 8, 8, 8, 0, 0],
        '8' => [14, 17, 17, 14, 17, 17, 14, 0, 0],
        '9' => [14, 17, 17, 15, 1, 1, 14, 0, 0],
        ':' => [0, 4, 4, 0, 4, 4, 0, 0, 0],
        ';' => [0, 4, 4, 0, 4, 4, 8, 0, 0],
        ',' => [0, 0, 0, 0, 0, 4, 4, 8, 0],
        '.' => [0, 0, 0, 0, 0, 4, 4, 0, 0],
        '-' => [0, 0, 0, 14, 0, 0, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0, 0, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0, 0, 0],
        '/' => [1, 1, 2, 4, 8, 16, 16, 0, 0],
        '"' => [10, 10, 0, 0, 0, 0, 0, 0, 0],
        '\'' => [4, 4, 8, 0, 0, 0, 0, 0, 0],
        '!' => [4, 4, 4, 4, 4, 0, 4, 0, 0],
        '?' => [14, 17, 1, 2, 4, 0, 4, 0, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2, 0, 0],
        ')' => [8, 4, 2, 2, 2, 4, 8, 0, 0],
        '[' => [14, 8, 8, 8, 8, 8, 14, 0, 0],
        ']' => [14, 2, 2, 2, 2, 2, 14, 0, 0],
        '<' => [2, 4, 8, 16, 8, 4, 2, 0, 0],
        '>' => [8, 4, 2, 1, 2, 4, 8, 0, 0],
        '#' => [10, 10, 31, 10, 31, 10, 10, 0, 0],
        '%' => [24, 25, 2, 4, 8, 19, 3, 0, 0],
        '*' => [0, 4, 21, 14, 21, 4, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31, 0, 0],
        '·' => [0, 0, 0, 4, 0, 0, 0, 0, 0],
        '×' => [0, 0, 17, 10, 4, 10, 17, 0, 0],
        '–' => [0, 0, 0, 31, 0, 0, 0, 0, 0],
        '…' => [0, 0, 0, 0, 0, 0, 21, 0, 0],
        '→' => [0, 4, 2, 31, 2, 4, 0, 0, 0],
        '▶' => [8, 12, 14, 15, 14, 12, 8, 0, 0],
        '‖' => [27, 27, 27, 27, 27, 27, 27, 0, 0],
        '■' => [0, 31, 31, 31, 31, 31, 0, 0, 0],
        ' ' => [0; 9],
        _ => [31, 17, 17, 17, 17, 17, 31, 0, 0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_breaks_at_spaces_and_keeps_long_words_whole() {
        assert_eq!(wrap("a wolf bit someone", 8), ["a wolf", "bit", "someone"]);
        assert_eq!(wrap("unbreakable", 4), ["unbreakable"]);
        assert!(wrap("", 4).is_empty());
    }

    #[test]
    fn width_counts_characters_not_bytes() {
        assert_eq!(text_width("a·b", 1.0), 17.0);
        assert_eq!(text_width("", 2.0), 0.0);
    }

    #[test]
    fn every_printable_ascii_character_used_in_labels_has_a_glyph() {
        let unknown = glyph_rows('\u{1}');
        for character in ('a'..='z').chain('A'..='Z').chain('0'..='9') {
            assert_ne!(glyph_rows(character), unknown, "{character}");
        }
    }
}
