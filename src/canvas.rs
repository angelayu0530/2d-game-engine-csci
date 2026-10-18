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

    pub fn number(&mut self, value: u32, x: f32, y: f32, cell: f32, color: u32) {
        let text = value.to_string();
        for (i, byte) in text.bytes().enumerate() {
            let glyph = DIGITS[usize::from(byte - b'0')];
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
}
