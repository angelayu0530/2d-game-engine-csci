/// 3x5 glyphs, one row per 3 bits, top row in the high bits.
const DIGITS: [u16; 10] = [
    0b111_101_101_101_111,
    0b010_110_010_010_111,
    0b111_001_111_100_111,
    0b111_001_111_001_111,
    0b101_101_111_001_001,
    0b111_100_111_001_111,
    0b111_100_111_101_111,
    0b111_001_001_001_001,
    0b111_101_111_101_111,
    0b111_101_111_001_111,
];

const LETTERS: [u16; 26] = [
    0b010_101_111_101_101, // A
    0b110_101_110_101_110, // B
    0b011_100_100_100_011, // C
    0b110_101_101_101_110, // D
    0b111_100_110_100_111, // E
    0b111_100_110_100_100, // F
    0b011_100_101_101_011, // G
    0b101_101_111_101_101, // H
    0b111_010_010_010_111, // I
    0b001_001_001_101_010, // J
    0b101_101_110_101_101, // K
    0b100_100_100_100_111, // L
    0b101_111_111_101_101, // M
    0b110_101_101_101_101, // N
    0b010_101_101_101_010, // O
    0b110_101_110_100_100, // P
    0b010_101_101_110_011, // Q
    0b110_101_110_101_101, // R
    0b011_100_010_001_110, // S
    0b111_010_010_010_010, // T
    0b101_101_101_101_111, // U
    0b101_101_101_101_010, // V
    0b101_101_111_111_101, // W
    0b101_101_010_101_101, // X
    0b101_101_010_010_010, // Y
    0b111_001_010_100_111, // Z
];

const UNKNOWN: u16 = 0b111_001_010_000_010;

fn glyph(c: char) -> u16 {
    match c {
        '0'..='9' => DIGITS[c as usize - '0' as usize],
        'A'..='Z' => LETTERS[c as usize - 'A' as usize],
        'a'..='z' => LETTERS[c as usize - 'a' as usize],
        ' ' => 0,
        '.' => 0b000_000_000_000_010,
        ',' => 0b000_000_000_010_100,
        '!' => 0b010_010_010_000_010,
        ':' => 0b000_010_000_010_000,
        ';' => 0b000_010_000_010_100,
        '/' => 0b001_001_010_100_100,
        '-' => 0b000_000_111_000_000,
        '+' => 0b000_010_111_010_000,
        '\'' => 0b010_010_000_000_000,
        '(' => 0b001_010_010_010_001,
        ')' => 0b100_010_010_010_100,
        '%' => 0b101_001_010_100_101,
        '#' => 0b101_111_101_111_101,
        '@' => 0b010_101_111_100_011,
        _ => UNKNOWN,
    }
}

pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let x0 = clip(x, self.width);
        let x1 = clip(x + w, self.width);
        let y0 = clip(y, self.height);
        let y1 = clip(y + h, self.height);
        for row in y0..y1 {
            self.pixels[row * self.width + x0..row * self.width + x1].fill(color);
        }
    }

    /// Sets the pixel containing `(x, y)`; points off the canvas (or NaN) are skipped.
    pub fn pixel(&mut self, x: f32, y: f32, color: u32) {
        if x >= 0.0 && y >= 0.0 {
            let (x, y) = (x as usize, y as usize);
            if x < self.width && y < self.height {
                self.pixels[y * self.width + x] = color;
            }
        }
    }

    pub fn number(&mut self, value: u32, x: f32, y: f32, cell: f32, color: u32) {
        self.text(&value.to_string(), x, y, cell, color);
    }

    /// Draws `text` in a 3x5 font, each glyph `cell` pixels per dot and
    /// advancing 4 cells. Letters render uppercase; unsupported characters
    /// render as `?`.
    pub fn text(&mut self, text: &str, x: f32, y: f32, cell: f32, color: u32) {
        for (i, c) in text.chars().enumerate() {
            let glyph = glyph(c);
            let left = x + i as f32 * 4.0 * cell;
            for bit in 0..15 {
                if glyph & (1 << (14 - bit)) != 0 {
                    let col = (bit % 3) as f32;
                    let row = (bit / 3) as f32;
                    self.fill_rect(left + col * cell, y + row * cell, cell, cell, color);
                }
            }
        }
    }

    /// Pixel width of `chars` glyphs drawn by [`Canvas::text`].
    pub fn text_width(chars: usize, cell: f32) -> f32 {
        (chars * 4).saturating_sub(1) as f32 * cell
    }
}

fn clip(v: f32, max: usize) -> usize {
    (v.max(0.0) as usize).min(max)
}

#[cfg(test)]
mod tests {
    use super::Canvas;

    #[test]
    fn fill_rect_clips_to_the_canvas() {
        let mut canvas = Canvas::new(4, 3);
        canvas.fill_rect(-2.0, 1.0, 4.0, 10.0, 7);
        canvas.fill_rect(3.0, -5.0, 9.0, 6.0, 9);
        #[rustfmt::skip]
        let expected = [
            0, 0, 0, 9,
            7, 7, 0, 0,
            7, 7, 0, 0,
        ];
        assert_eq!(canvas.pixels(), expected);
    }

    #[test]
    fn text_draws_letters_case_insensitively_and_advances_four_cells() {
        let mut lower = Canvas::new(8, 5);
        let mut upper = Canvas::new(8, 5);
        lower.text("hi", 0.0, 0.0, 1.0, 1);
        upper.text("HI", 0.0, 0.0, 1.0, 1);
        assert_eq!(lower.pixels(), upper.pixels());
        #[rustfmt::skip]
        let expected = [
            1, 0, 1, 0, 1, 1, 1, 0,
            1, 0, 1, 0, 0, 1, 0, 0,
            1, 1, 1, 0, 0, 1, 0, 0,
            1, 0, 1, 0, 0, 1, 0, 0,
            1, 0, 1, 0, 1, 1, 1, 0,
        ];
        assert_eq!(upper.pixels(), expected);
        assert_eq!(Canvas::text_width(2, 1.0), 7.0);
        assert_eq!(Canvas::text_width(0, 1.0), 0.0);
    }
}
