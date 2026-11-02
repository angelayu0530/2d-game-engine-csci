use std::io::{self, Cursor};

use crate::canvas::Canvas;

/// Clockwise quarter turns applied when drawing a tile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Turn {
    #[default]
    None,
    Quarter,
    Half,
    ThreeQuarter,
}

impl Turn {
    pub fn from_quarters(quarters: i32) -> Self {
        match quarters.rem_euclid(4) {
            0 => Turn::None,
            1 => Turn::Quarter,
            2 => Turn::Half,
            _ => Turn::ThreeQuarter,
        }
    }
}

/// How a tile is placed: rotated by `turn`, then mirrored horizontally if
/// `flip_x`, scaled by an integer `scale`, and optionally drawn as a solid
/// silhouette in `tint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    pub turn: Turn,
    pub flip_x: bool,
    pub scale: usize,
    pub tint: Option<u32>,
}

impl Default for Draw {
    fn default() -> Self {
        Self {
            turn: Turn::None,
            flip_x: false,
            scale: 1,
            tint: None,
        }
    }
}

/// Square tiles cut from a packed (no spacing) sprite sheet. Pixels are
/// `0xAARRGGBB`; alpha below 128 is transparent.
pub struct SpriteSheet {
    tile: usize,
    pixels: Vec<u32>,
}

impl SpriteSheet {
    /// Decodes an 8-bit RGB/RGBA or paletted PNG whose width and height are
    /// multiples of `tile`. Tiles are numbered row-major from the top left.
    pub fn from_png(bytes: &[u8], tile: usize) -> io::Result<Self> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::ALPHA | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(io::Error::other)?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| io::Error::other("png too large"))?;
        let mut buf = vec![0; size];
        let info = reader.next_frame(&mut buf).map_err(io::Error::other)?;
        let (width, height) = (info.width as usize, info.height as usize);
        if tile == 0 || width % tile != 0 || height % tile != 0 {
            return Err(io::Error::other(format!(
                "{width}x{height} sheet is not a grid of {tile}px tiles"
            )));
        }
        let channels = match info.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            other => {
                return Err(io::Error::other(format!(
                    "unsupported png color type {other:?}"
                )));
            }
        };
        let (columns, rows) = (width / tile, height / tile);
        let mut pixels = Vec::with_capacity(width * height);
        for index in 0..columns * rows {
            let (left, top) = (index % columns * tile, index / columns * tile);
            for y in top..top + tile {
                let row = &buf[y * info.line_size..];
                for x in left..left + tile {
                    let px = &row[x * channels..x * channels + channels];
                    let alpha = if channels == 4 { px[3] } else { 0xFF };
                    pixels.push(
                        u32::from(alpha) << 24
                            | u32::from(px[0]) << 16
                            | u32::from(px[1]) << 8
                            | u32::from(px[2]),
                    );
                }
            }
        }
        Ok(Self { tile, pixels })
    }

    pub fn tile_size(&self) -> usize {
        self.tile
    }

    pub fn len(&self) -> usize {
        self.pixels.len() / (self.tile * self.tile)
    }

    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// Tile `index` as row-major `0xAARRGGBB` pixels.
    pub fn tile(&self, index: usize) -> &[u32] {
        let area = self.tile * self.tile;
        &self.pixels[index * area..(index + 1) * area]
    }

    /// Draws tile `index` with its top-left corner at `(x, y)`.
    pub fn draw(&self, canvas: &mut Canvas, index: usize, x: f32, y: f32, how: Draw) {
        let tile = self.tile(index);
        let last = self.tile - 1;
        let scale = how.scale.max(1) as f32;
        for dy in 0..self.tile {
            for dx in 0..self.tile {
                let fx = if how.flip_x { last - dx } else { dx };
                let (sx, sy) = match how.turn {
                    Turn::None => (fx, dy),
                    Turn::Quarter => (dy, last - fx),
                    Turn::Half => (last - fx, last - dy),
                    Turn::ThreeQuarter => (last - dy, fx),
                };
                let color = tile[sy * self.tile + sx];
                if color >> 24 < 0x80 {
                    continue;
                }
                let color = how.tint.unwrap_or(color & 0x00FF_FFFF);
                let (px, py) = (x + dx as f32 * scale, y + dy as f32 * scale);
                if how.scale <= 1 {
                    canvas.pixel(px, py, color);
                } else {
                    canvas.fill_rect(px, py, scale, scale, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Draw, SpriteSheet, Turn};
    use crate::canvas::Canvas;

    fn sheet() -> SpriteSheet {
        // 2x2 RGBA tile: opaque red top-left, transparent elsewhere except
        // opaque blue bottom-left.
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            #[rustfmt::skip]
            writer.write_image_data(&[
                255, 0, 0, 255,   0, 0, 0, 0,
                0, 0, 255, 255,   0, 0, 0, 0,
            ]).unwrap();
        }
        SpriteSheet::from_png(&bytes, 2).unwrap()
    }

    fn drawn(how: Draw) -> Vec<u32> {
        let mut canvas = Canvas::new(2, 2);
        sheet().draw(&mut canvas, 0, 0.0, 0.0, how);
        canvas.pixels().to_vec()
    }

    #[test]
    fn transforms_place_pixels_where_a_clockwise_turn_and_mirror_put_them() {
        let (r, b) = (0xFF_00_00, 0x00_00_FF);
        assert_eq!(drawn(Draw::default()), [r, 0, b, 0]);
        let turn = |turn| Draw {
            turn,
            ..Draw::default()
        };
        assert_eq!(drawn(turn(Turn::Quarter)), [b, r, 0, 0]);
        assert_eq!(drawn(turn(Turn::Half)), [0, b, 0, r]);
        assert_eq!(drawn(turn(Turn::ThreeQuarter)), [0, 0, r, b]);
        let mirrored = Draw {
            flip_x: true,
            ..Draw::default()
        };
        assert_eq!(drawn(mirrored), [0, r, 0, b]);
    }

    #[test]
    fn bundled_tiny_dungeon_sheet_decodes_with_transparency() {
        let sheet = SpriteSheet::from_png(
            include_bytes!("../assets/tiny-dungeon/tilemap_packed.png"),
            16,
        )
        .unwrap();
        assert_eq!(sheet.len(), 12 * 11);
        let knight = sheet.tile(96);
        assert!(
            knight.iter().any(|&px| px >> 24 == 0),
            "corners transparent"
        );
        assert!(knight.iter().any(|&px| px >> 24 == 0xFF), "body opaque");
    }
}
