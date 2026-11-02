//! Draws a [`Fight`] with Kenney's CC0 Tiny Dungeon sprites
//! (`assets/tiny-dungeon/`) and a matching bow (`assets/bow.png`).

use std::f32::consts::FRAC_PI_4;

use super::{
    BOSS_HEARTS, BossMove, Fight, KNIGHT_HEARTS, Kind, ROOM, SHOT_COOLDOWN, Status, WIDTH,
};
use crate::{
    alloc::Allocator,
    canvas::Canvas,
    sprite::{Draw, SpriteSheet, Turn},
};

const SHEET: &[u8] = include_bytes!("../../assets/tiny-dungeon/tilemap_packed.png");
/// Tile 0 faces right, tile 1 faces down-right; quarter turns give the rest.
const BOW_SHEET: &[u8] = include_bytes!("../../assets/bow.png");
const TILE: f32 = 16.0;
const FLOOR: usize = 0;
const FLOOR_RUBBLE: usize = 24;
const WALL: usize = 40;
const BANNER: usize = 29;
const KNIGHT: usize = 96;
const BOSS: usize = 110;
const POTION: usize = 115;
const GRAVE: usize = 64;
const BOSS_SCALE: usize = 3;

const HUD: u32 = 0x1A_12_20;
const TEXT: u32 = 0xE8_E8_E8;
const DIM: u32 = 0x80_78_88;
const HEART: u32 = 0xE0_30_3A;
const HEART_EMPTY: u32 = 0x4A_2A_34;
const GOLD: u32 = 0xF0_C0_40;
const WHITE: u32 = 0xFF_FF_FF;
const TELEGRAPH: u32 = 0xFF_40_30;
const BOLT: u32 = 0xFF_5A_1E;
const BOLT_CORE: u32 = 0xFF_E0_66;
const ARROW_HEAD: u32 = 0xC0_CB_DC;
const ARROW_SHAFT: u32 = 0xEA_A5_6C;
const ARROW_FLETCH: u32 = 0xE8_E8_E8;
/// Recoil: the bow sits closer to the knight for this many steps after a shot.
const RECOIL_FRAMES: u32 = 5;
const SHADES: usize = 8;

const HINT: &str = "WASD/ARROWS MOVE  SPACE SHOOT  H HEAL  F THROW PARTICLES  R RESTART";

/// 7x6 heart, one row per byte, leftmost pixel in bit 6.
const HEART_ROWS: [u8; 6] = [
    0b0110110, 0b1111111, 0b1111111, 0b0111110, 0b0011100, 0b0001000,
];

pub struct Renderer {
    sheet: SpriteSheet,
    bow: SpriteSheet,
    palette: [[u32; SHADES]; Kind::ALL.len()],
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let sheet = SpriteSheet::from_png(SHEET, TILE as usize).expect("bundled sprite sheet");
        let bow = SpriteSheet::from_png(BOW_SHEET, TILE as usize).expect("bundled bow sprite");
        let palette = Kind::ALL.map(|kind| {
            let base = match kind {
                Kind::Ember => 0xFF_8A_3C,
                Kind::Shot => 0xF0_F4_FF,
                Kind::Spark => 0xFF_E0_66,
                Kind::Blood => 0xE0_30_3A,
                Kind::Heal => 0x5C_FF_7A,
                Kind::Fire => 0xFF_5A_1E,
                Kind::Spray => 0x6C_C8_FF,
                Kind::Blast => 0xFF_F0_C0,
            };
            std::array::from_fn(|shade| dim(base, 0.3 + 0.7 * shade as f32 / (SHADES - 1) as f32))
        });
        Self {
            sheet,
            bow,
            palette,
        }
    }

    pub fn draw<A: Allocator>(&self, fight: &Fight<A>, canvas: &mut Canvas) {
        canvas.clear(HUD);
        self.room(canvas);
        for (particle, kind) in fight.effects().iter() {
            let shade =
                (particle.lifetime as usize * SHADES / kind.max_life() as usize).min(SHADES - 1);
            canvas.pixel(
                particle.pos[0],
                particle.pos[1],
                self.palette[kind as usize][shade],
            );
        }
        for bolt in fight.bolts() {
            let [x, y] = bolt.pos;
            canvas.fill_rect(x - 2.5, y - 2.5, 5.0, 5.0, BOLT);
            canvas.fill_rect(x - 1.5, y - 1.5, 3.0, 3.0, BOLT_CORE);
        }
        for arrow in fight.arrows() {
            let speed = arrow.vel[0].hypot(arrow.vel[1]).max(1.0);
            let dir = [arrow.vel[0] / speed, arrow.vel[1] / speed];
            for (i, color) in [
                ARROW_HEAD,
                ARROW_HEAD,
                ARROW_SHAFT,
                ARROW_SHAFT,
                ARROW_SHAFT,
                ARROW_SHAFT,
                ARROW_FLETCH,
                ARROW_FLETCH,
            ]
            .into_iter()
            .enumerate()
            {
                let back = i as f32;
                canvas.pixel(
                    arrow.pos[0] - dir[0] * back,
                    arrow.pos[1] - dir[1] * back,
                    color,
                );
            }
        }
        self.boss(fight, canvas);
        self.knight(fight, canvas);
        self.hud(fight, canvas);
        banner(fight.status(), canvas);
    }

    fn room(&self, canvas: &mut Canvas) {
        let [left, top, right, bottom] = ROOM;
        let columns = (WIDTH / TILE) as usize;
        let wall_row = top / TILE - 1.0;
        for row in wall_row as usize..(bottom / TILE) as usize {
            for col in 0..columns {
                let (x, y) = (col as f32 * TILE, row as f32 * TILE);
                let tile = if x < left || x >= right || y < top {
                    if y < top && (col == 7 || col == columns - 8) {
                        BANNER
                    } else {
                        WALL
                    }
                } else if (col * 7 + row * 13) % 23 == 0 {
                    FLOOR_RUBBLE
                } else {
                    FLOOR
                };
                if tile == BANNER {
                    self.sheet.draw(canvas, WALL, x, y, Draw::default());
                }
                self.sheet.draw(canvas, tile, x, y, Draw::default());
            }
        }
    }

    fn boss<A: Allocator>(&self, fight: &Fight<A>, canvas: &mut Canvas) {
        let boss = fight.boss();
        if fight.status() == Status::Victory {
            return;
        }
        let tint = match boss.action {
            _ if boss.flash > 0 => Some(WHITE),
            BossMove::Telegraph { .. } if (fight.frame() / 4).is_multiple_of(2) => Some(TELEGRAPH),
            _ => None,
        };
        let half = TILE * BOSS_SCALE as f32 / 2.0;
        self.sheet.draw(
            canvas,
            BOSS,
            boss.pos[0] - half,
            boss.pos[1] - half,
            Draw {
                flip_x: fight.knight().pos[0] < boss.pos[0],
                scale: BOSS_SCALE,
                tint,
                ..Draw::default()
            },
        );
    }

    fn knight<A: Allocator>(&self, fight: &Fight<A>, canvas: &mut Canvas) {
        let knight = fight.knight();
        let [x, y] = knight.pos;
        if fight.status() == Status::Defeat {
            self.sheet
                .draw(canvas, GRAVE, x - 8.0, y - 8.0, Draw::default());
            return;
        }
        if knight.invulnerable / 4 % 2 == 1 {
            return;
        }
        let body = Draw {
            flip_x: knight.facing[0] < 0.0,
            ..Draw::default()
        };
        let [fx, fy] = knight.facing;
        let octant = (fy.atan2(fx) / FRAC_PI_4).round() as i32;
        let reach = if knight.cooldown > SHOT_COOLDOWN - RECOIL_FRAMES {
            4.0
        } else {
            7.0
        };
        let (bx, by) = (x + fx * reach - 8.0, y + fy * reach - 8.0);
        let bow = Draw {
            turn: Turn::from_quarters(octant.div_euclid(2)),
            ..Draw::default()
        };
        let tile = octant.rem_euclid(2) as usize;
        let behind = fy < 0.0;
        if behind {
            self.bow.draw(canvas, tile, bx, by, bow);
        }
        self.sheet.draw(canvas, KNIGHT, x - 8.0, y - 8.0, body);
        if !behind {
            self.bow.draw(canvas, tile, bx, by, bow);
        }
    }

    fn hud<A: Allocator>(&self, fight: &Fight<A>, canvas: &mut Canvas) {
        let knight = fight.knight();
        let boss = fight.boss();
        canvas.fill_rect(0.0, 0.0, WIDTH, ROOM[1] - TILE, HUD);
        canvas.fill_rect(0.0, ROOM[3], WIDTH, TILE, HUD);
        canvas.text("KNIGHT", 6.0, 6.0, 1.0, TEXT);
        for i in 0..KNIGHT_HEARTS {
            heart(canvas, 36.0 + i as f32 * 9.0, 5.0, i < knight.hp);
        }
        let potions_x = 36.0 + KNIGHT_HEARTS as f32 * 9.0 + 6.0;
        for i in 0..knight.potions {
            self.sheet.draw(
                canvas,
                POTION,
                potions_x + i as f32 * 10.0 - 3.0,
                0.0,
                Draw::default(),
            );
        }
        canvas.text("BOSS", 6.0, 21.0, 1.0, TEXT);
        for i in 0..BOSS_HEARTS {
            heart(canvas, 36.0 + i as f32 * 8.0, 20.0, i < boss.hp);
        }

        let live = format!("LIVE {}", fight.live_particles());
        let target = format!("TARGET {}", fight.config().particles);
        for (line, y, color) in [(live, 6.0, TEXT), (target, 21.0, DIM)] {
            let x = WIDTH - 6.0 - Canvas::text_width(line.len(), 1.0);
            canvas.text(&line, x, y, 1.0, color);
        }
        let x = (WIDTH - Canvas::text_width(HINT.len(), 1.0)) / 2.0;
        canvas.text(HINT, x.floor(), ROOM[3] + 5.0, 1.0, DIM);
    }
}

fn banner(status: Status, canvas: &mut Canvas) {
    let (title, color) = match status {
        Status::Fighting => return,
        Status::Victory => ("VICTORY", GOLD),
        Status::Defeat => ("YOU DIED", HEART),
    };
    let subtitle = "PRESS R TO FIGHT AGAIN";
    let cell = 4.0;
    let center = WIDTH / 2.0;
    canvas.fill_rect(center - 120.0, 136.0, 240.0, 56.0, HUD);
    let width = Canvas::text_width(title.len(), cell);
    canvas.text(title, (center - width / 2.0).floor(), 144.0, cell, color);
    let width = Canvas::text_width(subtitle.len(), 1.0);
    canvas.text(subtitle, (center - width / 2.0).floor(), 176.0, 1.0, TEXT);
}

fn heart(canvas: &mut Canvas, x: f32, y: f32, full: bool) {
    let color = if full { HEART } else { HEART_EMPTY };
    for (row, bits) in HEART_ROWS.iter().enumerate() {
        for col in 0..7 {
            if bits & 1 << (6 - col) != 0 {
                canvas.pixel(x + col as f32, y + row as f32, color);
            }
        }
    }
}

fn dim(color: u32, factor: f32) -> u32 {
    let channel =
        |shift: u32| (((color >> shift & 0xFF) as f32 * factor) as u32).min(0xFF) << shift;
    channel(16) | channel(8) | channel(0)
}
