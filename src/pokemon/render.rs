use std::collections::VecDeque;

use super::{Element, MAP, Monster, Pokemon, Species, State};
use crate::{
    alloc::Allocator,
    canvas::Canvas,
    sprite::{Draw, SpriteSheet},
};

pub const WIDTH: usize = 512;
pub const HEIGHT: usize = 384;
/// Characters per wrapped message line.
pub const LOG_CHARS: usize = 36;
/// Wrapped lines the log keeps. The field box shows this many.
pub const LOG_LINES: usize = 6;

const SHEET: &[u8] = include_bytes!("../../assets/pokemon/sprites.png");
const TILE_PX: usize = 16;
const TILE: f32 = 32.0;
const MAP_X: f32 = 64.0;
const MAP_Y: f32 = 4.0;

const PATH: usize = 0;
const GRASS: usize = 1;
const WALL: usize = 2;
const CENTER: usize = 3;
const TRAINER_DOWN: usize = 8;
const TRAINER_UP: usize = 9;
const TRAINER_LEFT: usize = 10;
const TRAINER_RIGHT: usize = 11;
const PSYDUCK_FRONT: usize = 12;
const PSYDUCK_BACK: usize = 13;
const POLIWAG_FRONT: usize = 14;
const POLIWAG_BACK: usize = 15;
const CHARMANDER_FRONT: usize = 16;
const CHARMANDER_BACK: usize = 17;
const BULBASAUR_FRONT: usize = 18;
const BULBASAUR_BACK: usize = 19;
const SQUIRTLE_FRONT: usize = 20;
const SQUIRTLE_BACK: usize = 21;
const VULPIX_FRONT: usize = 22;
const VULPIX_BACK: usize = 23;
const BALL: usize = 24;
const GROWLITHE_FRONT: usize = 25;
const GROWLITHE_BACK: usize = 26;
const ODDISH_FRONT: usize = 27;
const ODDISH_BACK: usize = 28;
const BELLSPROUT_FRONT: usize = 29;
const BELLSPROUT_BACK: usize = 30;

const BORDER: u32 = 0x18_20_28;
const CREAM: u32 = 0xF8_F8_F0;
const INK: u32 = 0x20_20_28;
const DIM: u32 = 0x68_68_70;
const BLACK: u32 = 0x00_00_00;
const HINT: u32 = 0xC8_C8_D0;
const SKY: u32 = 0xB8_E0_F8;
const GROUND: u32 = 0xC8_B0_70;
const OVAL: u32 = 0xE4_D4_A4;
const TRACK: u32 = 0x30_30_38;
const EMBER: u32 = 0xC0_30_10;
const SPARK: u32 = 0xFF_E0_60;

const FIELD_SCALE: usize = 2;
const BATTLE_SCALE: usize = 4;
const LOG_CELL: f32 = 2.0;
const LOG_STEP: f32 = 16.0;

const HINT_LINE: &str = "ARROWS MOVE  H HEALS ON CENTER  ESC QUITS";

/// Message history, word-wrapped to [`LOG_CHARS`] and capped at [`LOG_LINES`].
/// Lines pushed since the last [`Log::next_turn`] draw in the newest-turn color.
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Facing {
    Up,
    #[default]
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BattleCommand {
    #[default]
    Fight,
    Catch,
    Pokemon,
    Run,
}

#[derive(Default)]
pub struct Hud {
    pub log: Log,
    pub facing: Facing,
    pub command: BattleCommand,
    pub party_open: bool,
    pub party_slot: usize,
}

const COMMANDS: [(BattleCommand, &str); 4] = [
    (BattleCommand::Fight, "FIGHT"),
    (BattleCommand::Catch, "CATCH"),
    (BattleCommand::Pokemon, "PKMN"),
    (BattleCommand::Run, "RUN"),
];

pub struct Renderer {
    sheet: SpriteSheet,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            sheet: SpriteSheet::from_png(SHEET, TILE_PX).expect("pokemon sprites"),
        }
    }

    pub fn draw<A: Allocator>(&self, game: &Pokemon<A>, hud: &Hud, canvas: &mut Canvas) {
        if game.state().wild.is_some() {
            self.battle(game, hud, canvas);
        } else {
            self.field(game, hud, canvas);
        }
        if hud.party_open {
            party(canvas, game.state(), hud);
        }
    }

    fn field<A: Allocator>(&self, game: &Pokemon<A>, hud: &Hud, canvas: &mut Canvas) {
        let state = game.state();
        canvas.clear(BORDER);
        let how = Draw {
            scale: FIELD_SCALE,
            ..Draw::default()
        };
        for (row, line) in MAP.iter().enumerate() {
            for (col, tile) in line.bytes().enumerate() {
                let index = match tile {
                    b'#' => WALL,
                    b'g' => GRASS,
                    b'H' => CENTER,
                    _ => PATH,
                };
                self.sheet.draw(
                    canvas,
                    index,
                    MAP_X + col as f32 * TILE,
                    MAP_Y + row as f32 * TILE,
                    how,
                );
            }
        }
        sparks(game, canvas);
        let [col, row] = state.position;
        self.sheet.draw(
            canvas,
            trainer(hud.facing),
            MAP_X + col as f32 * TILE,
            MAP_Y + row as f32 * TILE,
            how,
        );
        if let Some(pending) = state.pending {
            self.choice(canvas, pending);
        }
        panel(canvas, 8.0, 268.0, 496.0, 108.0);
        messages(canvas, &hud.log, 20.0, 280.0, LOG_LINES);
        canvas.text(HINT_LINE, 20.0, 378.0, 1.0, HINT);
    }

    fn battle<A: Allocator>(&self, game: &Pokemon<A>, hud: &Hud, canvas: &mut Canvas) {
        let state = game.state();
        let Some(wild) = state.wild else {
            return;
        };
        canvas.clear(BORDER);
        canvas.fill_rect(0.0, 0.0, WIDTH as f32, 200.0, SKY);
        canvas.fill_rect(0.0, 200.0, WIDTH as f32, 68.0, GROUND);
        fill_oval(canvas, 88.0, 206.0, 42.0, 12.0, OVAL);
        let monster = Draw {
            scale: BATTLE_SCALE,
            ..Draw::default()
        };
        self.sheet.draw(
            canvas,
            sprite_for(wild.species, false),
            360.0,
            28.0,
            monster,
        );
        self.sheet.draw(
            canvas,
            sprite_for(state.starter.species, true),
            56.0,
            132.0,
            monster,
        );
        status(canvas, 12.0, 10.0, 220.0, wild, false);
        status(canvas, 260.0, 202.0, 236.0, state.starter, true);
        panel(canvas, 8.0, 276.0, 324.0, 100.0);
        messages(canvas, &hud.log, 20.0, 282.0, LOG_LINES);
        self.commands(canvas, hud.command);
    }

    fn commands(&self, canvas: &mut Canvas, selected: BattleCommand) {
        panel(canvas, 340.0, 276.0, 164.0, 100.0);
        let left = 344.0;
        let top = 280.0;
        let col_w = 78.0;
        let row_h = 46.0;
        for (i, (command, label)) in COMMANDS.iter().enumerate() {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            let x = left + col * col_w;
            let y = top + row * row_h;
            let on = *command == selected;
            if on {
                canvas.fill_rect(x, y, col_w, row_h, BLACK);
            }
            let text_x = x + 8.0;
            canvas.text(
                label,
                text_x,
                y + 18.0,
                LOG_CELL,
                if on { CREAM } else { INK },
            );
            if *command == BattleCommand::Catch {
                self.sheet.draw(
                    canvas,
                    BALL,
                    text_x + Canvas::text_width(label.len(), LOG_CELL) + 4.0,
                    y + 15.0,
                    Draw {
                        scale: 1,
                        ..Draw::default()
                    },
                );
            }
        }
        canvas.fill_rect(left + col_w - 1.0, top, 2.0, row_h * 2.0, BLACK);
        canvas.fill_rect(left, top + row_h - 1.0, col_w * 2.0, 2.0, BLACK);
    }

    fn choice(&self, canvas: &mut Canvas, monster: Monster) {
        let x = 144.0;
        let y = 96.0;
        panel(canvas, x, y, 224.0, 72.0);
        self.sheet.draw(
            canvas,
            sprite_for(monster.species, false),
            x + 12.0,
            y + 20.0,
            Draw {
                scale: FIELD_SCALE,
                ..Draw::default()
            },
        );
        let text_x = x + 52.0;
        canvas.text(monster.name(), text_x, y + 16.0, LOG_CELL, INK);
        canvas.text("1-5 DROP ONE TO KEEP", text_x, y + 32.0, LOG_CELL, INK);
        canvas.text("RUN LETS IT GO", text_x, y + 48.0, LOG_CELL, INK);
    }
}

fn trainer(facing: Facing) -> usize {
    match facing {
        Facing::Up => TRAINER_UP,
        Facing::Down => TRAINER_DOWN,
        Facing::Left => TRAINER_LEFT,
        Facing::Right => TRAINER_RIGHT,
    }
}

fn sprite_for(species: Species, back: bool) -> usize {
    match (species, back) {
        (Species::Charmander, false) => CHARMANDER_FRONT,
        (Species::Charmander, true) => CHARMANDER_BACK,
        (Species::Vulpix, false) => VULPIX_FRONT,
        (Species::Vulpix, true) => VULPIX_BACK,
        (Species::Growlithe, false) => GROWLITHE_FRONT,
        (Species::Growlithe, true) => GROWLITHE_BACK,
        (Species::Bulbasaur, false) => BULBASAUR_FRONT,
        (Species::Bulbasaur, true) => BULBASAUR_BACK,
        (Species::Oddish, false) => ODDISH_FRONT,
        (Species::Oddish, true) => ODDISH_BACK,
        (Species::Bellsprout, false) => BELLSPROUT_FRONT,
        (Species::Bellsprout, true) => BELLSPROUT_BACK,
        (Species::Squirtle, false) => SQUIRTLE_FRONT,
        (Species::Squirtle, true) => SQUIRTLE_BACK,
        (Species::Psyduck, false) => PSYDUCK_FRONT,
        (Species::Psyduck, true) => PSYDUCK_BACK,
        (Species::Poliwag, false) => POLIWAG_FRONT,
        (Species::Poliwag, true) => POLIWAG_BACK,
    }
}

fn sparks<A: Allocator>(game: &Pokemon<A>, canvas: &mut Canvas) {
    let max_life = game.config().lifetime.max(1) as f32;
    let right = MAP_X + MAP[0].len() as f32 * TILE;
    let bottom = MAP_Y + MAP.len() as f32 * TILE;
    for particle in game.particles() {
        let x = particle.pos[0] + MAP_X + TILE * 0.5;
        let y = particle.pos[1] + MAP_Y + TILE * 0.5;
        if (MAP_X + 1.0..right - 1.0).contains(&x) && (MAP_Y + 1.0..bottom - 1.0).contains(&y) {
            let color = mix(EMBER, SPARK, particle.lifetime as f32 / max_life);
            canvas.fill_rect(x - 1.0, y - 1.0, 2.0, 2.0, color);
        }
    }
}

fn messages(canvas: &mut Canvas, log: &Log, x: f32, y: f32, rows: usize) {
    let skip = log.lines.len().saturating_sub(rows);
    for (i, (turn, line)) in log.lines.iter().skip(skip).enumerate() {
        let color = if *turn == log.turn { INK } else { DIM };
        canvas.text(line, x, y + i as f32 * LOG_STEP, LOG_CELL, color);
    }
}

fn party(canvas: &mut Canvas, state: &State, hud: &Hud) {
    let entries = state.collection.len();
    let rows = entries.max(1);
    let width = 288.0;
    let height = 36.0 + rows as f32 * 18.0;
    let x = 16.0;
    let y = 72.0;
    panel(canvas, x, y, width, height);
    canvas.text("PARTY", x + 12.0, y + 8.0, LOG_CELL, INK);
    if entries == 0 {
        canvas.text("NONE", x + 12.0, y + 28.0, LOG_CELL, INK);
        return;
    }
    let slot = hud.party_slot.min(entries - 1);
    for (i, monster) in state.collection.iter().enumerate() {
        let row_y = y + 28.0 + i as f32 * 18.0;
        let label = format!(
            "{} {} LV{} {}/{}",
            i + 1,
            monster.name(),
            monster.level,
            monster.hp,
            monster.max_hp()
        );
        let color = if i == slot {
            canvas.fill_rect(x + 8.0, row_y - 2.0, width - 16.0, 16.0, BLACK);
            CREAM
        } else if monster.hp == 0 {
            DIM
        } else {
            INK
        };
        canvas.text(&label, x + 12.0, row_y, LOG_CELL, color);
    }
}

fn status(canvas: &mut Canvas, x: f32, y: f32, w: f32, monster: Monster, xp: bool) {
    let h = 58.0;
    panel(canvas, x, y, w, h);
    canvas.fill_rect(
        x + 4.0,
        y + 4.0,
        4.0,
        h - 8.0,
        element_color(monster.element()),
    );
    let text_x = x + 14.0;
    canvas.text(monster.name(), text_x, y + 8.0, LOG_CELL, INK);
    let level = format!("LV {}", monster.level);
    let level_w = Canvas::text_width(level.len(), LOG_CELL);
    canvas.text(
        &level,
        x + w - 8.0 - level_w,
        y + 8.0,
        LOG_CELL,
        element_color(monster.element()),
    );
    let bar_w = w - 24.0;
    let bar_y = y + 26.0;
    let max_hp = monster.max_hp().max(1);
    let frac = (monster.hp as f32 / max_hp as f32).clamp(0.0, 1.0);
    canvas.fill_rect(text_x, bar_y, bar_w, 8.0, TRACK);
    canvas.fill_rect(
        text_x,
        bar_y,
        bar_w * frac,
        8.0,
        hp_color(monster.hp, max_hp),
    );
    canvas.text(
        &format!("{}/{}", monster.hp, monster.max_hp()),
        text_x,
        y + 40.0,
        LOG_CELL,
        INK,
    );
    if xp {
        let label = format!("XP {}/{}", monster.xp, monster.level.saturating_mul(20));
        let label_w = Canvas::text_width(label.len(), LOG_CELL);
        canvas.text(&label, x + w - 8.0 - label_w, y + 40.0, LOG_CELL, DIM);
    }
}

fn panel(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    canvas.fill_rect(x, y, w, h, BLACK);
    canvas.fill_rect(x + 4.0, y + 4.0, w - 8.0, h - 8.0, CREAM);
}

fn fill_oval(canvas: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, color: u32) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let steps = ry.ceil() as i32;
    for dy in -steps..=steps {
        let y = dy as f32;
        let t = 1.0 - (y / ry) * (y / ry);
        if t <= 0.0 {
            continue;
        }
        let half = rx * t.sqrt();
        canvas.fill_rect(cx - half, cy + y, half * 2.0, 1.0, color);
    }
}

fn element_color(element: Element) -> u32 {
    match element {
        Element::Fire => 0xF0_78_30,
        Element::Grass => 0x48_B0_38,
        Element::Water => 0x40_90_E0,
    }
}

fn hp_color(hp: u32, max_hp: u32) -> u32 {
    match hp.saturating_mul(4) / max_hp.max(1) {
        0 => 0xD0_30_28,
        1 => 0xE0_B8_20,
        _ => 0x30_C0_48,
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
    use super::{Facing, HEIGHT, Hud, LOG_CHARS, LOG_LINES, Log, Renderer, WIDTH};
    use crate::{
        alloc::pool::PoolAllocator,
        canvas::Canvas,
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
        let renderer = Renderer::new();
        let hud = Hud {
            facing: Facing::Down,
            ..Hud::default()
        };
        renderer.draw(&game, &hud, &mut canvas);

        const HAT: u32 = 0xE0_40_40;
        let hat_at = |col: f32, row: f32| (64.0 + col * 32.0 + 14.0, 4.0 + row * 32.0 + 4.0);
        let (x, y) = hat_at(1.0, 1.0);
        assert_eq!(at(&canvas, x, y), HAT);
        let (x, y) = hat_at(2.0, 1.0);
        assert_ne!(at(&canvas, x, y), HAT);

        game.step(Action::Right);
        renderer.draw(&game, &hud, &mut canvas);
        let (x, y) = hat_at(2.0, 1.0);
        assert_eq!(at(&canvas, x, y), HAT);
        let (x, y) = hat_at(1.0, 1.0);
        assert_ne!(at(&canvas, x, y), HAT);
        assert_ne!(at(&canvas, 64.0 + 96.0 + 4.0, 4.0 + 32.0 + 4.0), HAT);
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
