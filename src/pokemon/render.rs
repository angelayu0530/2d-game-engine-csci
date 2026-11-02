use std::collections::VecDeque;

use super::{Element, MAP, Monster, Pokemon, State};
use crate::{alloc::Allocator, canvas::Canvas};

pub const TILE: f32 = 32.0;
pub const WIDTH: usize = 512;
pub const HEIGHT: usize = 384;

const MAP_W: f32 = MAP[0].len() as f32 * TILE;
const MAP_H: f32 = MAP.len() as f32 * TILE;
const CELL: f32 = 1.0;
const LINE: f32 = 7.0;
const PANEL_X: f32 = MAP_W + 6.0;
const LOG_X: f32 = 4.0;
const LOG_TOP: f32 = MAP_H + 6.0;
const HINT_TOP: f32 = HEIGHT as f32 - 2.0 * LINE - 2.0;
/// Characters per message line under the map.
pub const LOG_CHARS: usize = ((MAP_W - 2.0 * LOG_X) / (4.0 * CELL)) as usize;
/// Message lines kept and shown under the map.
pub const LOG_LINES: usize = ((HINT_TOP - LOG_TOP) / LINE) as usize;

pub const BACKGROUND: u32 = 0x14_16_1C;
const TEXT: u32 = 0xE8_E8_E8;
const DIM: u32 = 0x80_84_8C;
const HEADER: u32 = 0xF0_C0_40;
const PATH: u32 = 0xC8_B4_80;
const GRASS: u32 = 0x3C_8C_3C;
const GRASS_TUFT: u32 = 0x26_62_26;
const WALL: u32 = 0x4C_4C_58;
const WALL_SHADE: u32 = 0x34_34_3E;
const CENTER: u32 = 0xF0_F0_F4;
const CROSS: u32 = 0xD0_30_30;
const OUTLINE: u32 = 0x18_18_20;
const FACE: u32 = 0xF0_D0_B0;
const SPARK: u32 = 0xFF_E0_60;
const EMBER: u32 = 0xC0_30_10;
const BAR_BACK: u32 = 0x30_32_3A;

const HINTS: [&str; 2] = [
    "ARROWS/WASD MOVE  F ATTACK  C CATCH  R RUN  H HEAL AT H",
    "1-5 SWITCH  SHIFT+1-5 RELEASE  ESC SAVE AND QUIT",
];

/// Message history, word-wrapped to [`LOG_CHARS`] and capped at [`LOG_LINES`].
/// Lines pushed since the last [`Log::next_turn`] draw highlighted.
#[derive(Debug, Default)]
pub struct Log {
    lines: VecDeque<(u64, String)>,
    turn: u64,
}

impl Log {
    pub fn next_turn(&mut self) {
        self.turn += 1;
    }

    pub fn push(&mut self, message: &str) {
        let mut line = String::new();
        let mut len = 0;
        for word in message.split_whitespace() {
            let n = word.chars().count();
            if len > 0 && len + 1 + n > LOG_CHARS {
                self.line(std::mem::take(&mut line));
                len = 0;
            }
            if len > 0 {
                line.push(' ');
                len += 1;
            }
            line.push_str(word);
            len += n;
        }
        if len > 0 {
            self.line(line);
        }
    }

    fn line(&mut self, line: String) {
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back((self.turn, line));
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|(_, line)| line.as_str())
    }
}

pub fn draw<A: Allocator>(game: &Pokemon<A>, log: &Log, canvas: &mut Canvas) {
    let state = game.state();
    canvas.clear(BACKGROUND);
    tiles(canvas);

    let max_life = game.config().lifetime.max(1) as f32;
    for particle in game.particles() {
        let x = particle.pos[0] + TILE / 2.0;
        let y = particle.pos[1] + TILE / 2.0;
        if (1.0..MAP_W - 1.0).contains(&x) && (1.0..MAP_H - 1.0).contains(&y) {
            let color = mix(EMBER, SPARK, particle.lifetime as f32 / max_life);
            canvas.fill_rect(x - 1.0, y - 1.0, 2.0, 2.0, color);
        }
    }

    player(canvas, state);
    panel(canvas, state);

    for (row, (turn, line)) in log.lines.iter().enumerate() {
        let color = if *turn == log.turn { TEXT } else { DIM };
        canvas.text(line, LOG_X, LOG_TOP + row as f32 * LINE, CELL, color);
    }
    for (row, hint) in HINTS.iter().enumerate() {
        canvas.text(hint, LOG_X, HINT_TOP + row as f32 * LINE, CELL, HEADER);
    }
}

fn tiles(canvas: &mut Canvas) {
    for (y, row) in MAP.iter().enumerate() {
        for (x, tile) in row.bytes().enumerate() {
            let (px, py) = (x as f32 * TILE, y as f32 * TILE);
            match tile {
                b'#' => {
                    canvas.fill_rect(px, py, TILE, TILE, WALL);
                    canvas.fill_rect(px, py + TILE - 4.0, TILE, 4.0, WALL_SHADE);
                }
                b'g' => {
                    canvas.fill_rect(px, py, TILE, TILE, GRASS);
                    for (dx, dy) in [(5.0, 6.0), (18.0, 3.0), (10.0, 19.0), (24.0, 16.0)] {
                        canvas.fill_rect(px + dx, py + dy, 2.0, 7.0, GRASS_TUFT);
                        canvas.fill_rect(px + dx + 3.0, py + dy + 2.0, 2.0, 5.0, GRASS_TUFT);
                    }
                }
                b'H' => {
                    canvas.fill_rect(px, py, TILE, TILE, PATH);
                    canvas.fill_rect(px + 4.0, py + 4.0, TILE - 8.0, TILE - 8.0, CENTER);
                    canvas.fill_rect(px + 13.0, py + 8.0, 6.0, 16.0, CROSS);
                    canvas.fill_rect(px + 8.0, py + 13.0, 16.0, 6.0, CROSS);
                }
                _ => canvas.fill_rect(px, py, TILE, TILE, PATH),
            }
        }
    }
}

fn player(canvas: &mut Canvas, state: &State) {
    let px = state.position[0] as f32 * TILE;
    let py = state.position[1] as f32 * TILE;
    canvas.fill_rect(px + 7.0, py + 5.0, 18.0, 22.0, OUTLINE);
    canvas.fill_rect(px + 8.0, py + 6.0, 16.0, 6.0, CROSS);
    canvas.fill_rect(px + 8.0, py + 12.0, 16.0, 8.0, FACE);
    canvas.fill_rect(px + 11.0, py + 14.0, 2.0, 2.0, OUTLINE);
    canvas.fill_rect(px + 19.0, py + 14.0, 2.0, 2.0, OUTLINE);
    canvas.fill_rect(
        px + 8.0,
        py + 20.0,
        16.0,
        6.0,
        element_color(state.starter.element),
    );
    if state.wild.is_some() {
        canvas.fill_rect(px + 12.0, py - 13.0, 8.0, 14.0, OUTLINE);
        canvas.text("!", px + 13.0, py - 12.0, 2.0, HEADER);
    }
}

fn panel(canvas: &mut Canvas, state: &State) {
    let x = PANEL_X;
    let mut y = 4.0;
    let line = |canvas: &mut Canvas, text: &str, color: u32, y: &mut f32| {
        canvas.text(text, x, *y, CELL, color);
        *y += LINE;
    };

    line(canvas, "ACTIVE", HEADER, &mut y);
    monster(canvas, x, &mut y, state.starter);
    line(
        canvas,
        &format!("XP {}/{}", state.starter.xp, state.starter.level * 20),
        TEXT,
        &mut y,
    );
    y += 4.0;

    if let Some(wild) = state.wild {
        line(canvas, "WILD", HEADER, &mut y);
        monster(canvas, x, &mut y, wild);
        line(canvas, "F ATTACK C CATCH R RUN", DIM, &mut y);
        y += 4.0;
    }

    line(canvas, "PARTY", HEADER, &mut y);
    if state.collection.is_empty() {
        line(canvas, "NONE YET", DIM, &mut y);
    }
    for (i, m) in state.collection.iter().enumerate() {
        let color = if m.hp == 0 {
            DIM
        } else {
            element_color(m.element)
        };
        let text = format!(
            "{} {} L{} {}/{}",
            i + 1,
            m.name(),
            m.level,
            m.hp,
            m.max_hp()
        );
        line(canvas, &text, color, &mut y);
    }
    y += 4.0;

    line(canvas, &format!("BALLS {}", state.balls), TEXT, &mut y);
    line(
        canvas,
        &format!("WINS {} CATCHES {}", state.wins, state.catches),
        TEXT,
        &mut y,
    );
    line(canvas, &format!("LOSSES {}", state.losses), TEXT, &mut y);
    y += 4.0;

    line(canvas, "GOAL", HEADER, &mut y);
    let mut gx = x;
    for (element, name) in [
        (Element::Fire, "FIRE"),
        (Element::Grass, "GRASS"),
        (Element::Water, "WATER"),
    ] {
        let owned = std::iter::once(&state.starter)
            .chain(&state.collection)
            .any(|m| m.element == element);
        let color = if owned { element_color(element) } else { DIM };
        canvas.text(name, gx, y, CELL, color);
        gx += Canvas::text_width(name.len() + 1, CELL) + CELL;
    }
    if state.has_every_element() {
        canvas.text("COMPLETE!", x, y + LINE, CELL, HEADER);
    }
}

fn monster(canvas: &mut Canvas, x: f32, y: &mut f32, m: Monster) {
    canvas.fill_rect(x, *y, 5.0, 5.0, element_color(m.element));
    canvas.text(m.name(), x + 8.0, *y, CELL, TEXT);
    *y += LINE;
    canvas.text(
        &format!("LV {} {:?}", m.level, m.element),
        x,
        *y,
        CELL,
        TEXT,
    );
    *y += LINE;
    let width = WIDTH as f32 - x - 8.0;
    let fill = width * m.hp as f32 / m.max_hp() as f32;
    canvas.fill_rect(x, *y, width, 4.0, BAR_BACK);
    canvas.fill_rect(x, *y, fill, 4.0, hp_color(m));
    *y += 6.0;
    canvas.text(&format!("HP {}/{}", m.hp, m.max_hp()), x, *y, CELL, TEXT);
    *y += LINE;
}

fn element_color(element: Element) -> u32 {
    match element {
        Element::Fire => 0xF0_70_30,
        Element::Grass => 0x60_C8_50,
        Element::Water => 0x50_90_F0,
    }
}

fn hp_color(m: Monster) -> u32 {
    match m.hp * 4 / m.max_hp() {
        0 => 0xE0_40_30,
        1 => 0xE0_C0_30,
        _ => 0x50_D0_60,
    }
}

/// Linear blend from `a` (t = 0) to `b` (t = 1), per channel.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let from = (a >> shift & 0xFF) as f32;
        let to = (b >> shift & 0xFF) as f32;
        ((from + (to - from) * t) as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::{CENTER, LOG_CHARS, LOG_LINES, Log, OUTLINE, PATH, TILE, draw};
    use crate::{
        alloc::pool::PoolAllocator,
        canvas::Canvas,
        pokemon::render::{HEIGHT, WIDTH},
        pokemon::{Action, Config, Pokemon},
    };

    fn at(canvas: &Canvas, x: f32, y: f32) -> u32 {
        canvas.pixels()[y as usize * canvas.width() + x as usize]
    }

    #[test]
    fn player_is_drawn_on_its_tile_and_leaves_the_previous_one() {
        let mut game = Pokemon::<PoolAllocator>::new(Config {
            burst: 0,
            ..Config::default()
        });
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        draw(&game, &Log::default(), &mut canvas);
        assert_eq!(at(&canvas, TILE + 7.0, TILE + 5.0), OUTLINE);
        assert_eq!(at(&canvas, 2.0 * TILE + 7.0, TILE + 5.0), PATH);

        game.step(Action::Right);
        draw(&game, &Log::default(), &mut canvas);
        assert_eq!(at(&canvas, 2.0 * TILE + 7.0, TILE + 5.0), OUTLINE);
        assert_eq!(at(&canvas, TILE + 5.0, TILE + 5.0), CENTER);
        assert_eq!(at(&canvas, 2.0 * TILE + 7.0, 2.0 * TILE + 2.0), PATH);
    }

    #[test]
    fn log_wraps_at_word_boundaries_and_drops_oldest_lines() {
        let mut log = Log::default();
        let word = "x".repeat((LOG_CHARS - 1) / 2);
        log.push(&format!("{word} {word} {word}"));
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines, [format!("{word} {word}"), word.clone()]);
        assert!(lines.iter().all(|l| l.chars().count() <= LOG_CHARS));

        for i in 0..LOG_LINES {
            log.push(&i.to_string());
        }
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines[0], "0");
        assert_eq!(lines[LOG_LINES - 1], (LOG_LINES - 1).to_string());
    }
}
