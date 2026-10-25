use super::*;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or unsupported Pokemon save",
    )
}
struct Words<'a>(std::str::SplitWhitespace<'a>);
impl Words<'_> {
    fn number<T: std::str::FromStr>(&mut self) -> io::Result<T> {
        self.0
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())
    }
    fn monster(&mut self) -> io::Result<Monster> {
        let element = match self.number::<u8>()? {
            0 => Element::Fire,
            1 => Element::Grass,
            2 => Element::Water,
            _ => return Err(invalid()),
        };
        let m = Monster {
            element,
            level: self.number()?,
            hp: self.number()?,
            xp: self.number()?,
        };
        if !(1..=100).contains(&m.level) || m.hp > m.max_hp() {
            return Err(invalid());
        }
        Ok(m)
    }
}
impl<A: Allocator> Pokemon<A> {
    pub fn snapshot(&self) -> String {
        let mut text = format!(
            "POKEMON_SAVE_2\n{} {} {} {} {}\n{} {} {} {} {} {} {}\n",
            self.cfg.seed,
            self.cfg.burst,
            self.cfg.lifetime,
            self.cfg.trail,
            self.cfg.dt,
            self.rng,
            self.frame,
            self.created,
            self.retired,
            self.updates,
            self.peak_live,
            self.peak_reserved
        );
        let s = &self.state;
        text.push_str(&format!(
            "{} {} {} {} {} {} {}\n",
            s.position[0], s.position[1], s.balls, s.encounters, s.wins, s.losses, s.catches
        ));
        fn monster(text: &mut String, m: Monster) {
            text.push_str(&format!(
                "{} {} {} {}\n",
                m.element as u8, m.level, m.hp, m.xp
            ));
        }
        monster(&mut text, s.starter);
        text.push_str(&format!("{}\n", u8::from(s.wild.is_some())));
        if let Some(m) = s.wild {
            monster(&mut text, m);
        }
        text.push_str(&format!("{}\n", s.collection.len()));
        for &m in &s.collection {
            monster(&mut text, m);
        }
        text.push_str(&format!("{}\n", self.handles.len()));
        for &handle in &self.handles {
            let p = self.alloc.get(handle);
            text.push_str(&format!(
                "{} {} {} {} {}\n",
                p.pos[0].to_bits(),
                p.pos[1].to_bits(),
                p.vel[0].to_bits(),
                p.vel[1].to_bits(),
                p.lifetime
            ));
        }
        text
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = self.snapshot();
        let mut temporary = path.as_os_str().to_os_string();
        temporary.push(format!(".{}.tmp", std::process::id()));
        let temporary = std::path::PathBuf::from(temporary);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
    pub fn load(path: &Path) -> io::Result<Self> {
        Self::from_snapshot(&fs::read_to_string(path)?)
    }

    pub fn from_snapshot(text: &str) -> io::Result<Self> {
        let mut words = Words(text.split_whitespace());
        let version = words.0.next();
        if !matches!(version, Some("POKEMON_SAVE_1" | "POKEMON_SAVE_2")) {
            return Err(invalid());
        }
        let cfg = Config {
            seed: words.number()?,
            burst: words.number()?,
            lifetime: words.number()?,
            trail: if version == Some("POKEMON_SAVE_2") {
                words.number()?
            } else {
                0
            },
            dt: if version == Some("POKEMON_SAVE_2") {
                words.number()?
            } else {
                1.0 / 60.0
            },
        };
        if cfg.lifetime == 0 || !cfg.dt.is_finite() || cfg.dt <= 0.0 {
            return Err(invalid());
        }
        let mut game = Self::new(cfg);
        game.rng = words.number()?;
        game.frame = words.number()?;
        game.created = words.number()?;
        game.retired = words.number()?;
        game.updates = words.number()?;
        game.peak_live = words.number()?;
        game.peak_reserved = words.number()?;
        let position: [usize; 2] = [words.number()?, words.number()?];
        let balls = words.number()?;
        let encounters = words.number()?;
        let wins = words.number()?;
        let losses = words.number()?;
        let catches = words.number()?;
        let starter = words.monster()?;
        let wild = match words.number::<u8>()? {
            0 => None,
            1 => Some(words.monster()?),
            _ => return Err(invalid()),
        };
        let count = words.number::<usize>()?;
        if count > 5
            || balls > 10
            || starter.hp == 0
            || wild.is_some_and(|m| m.hp == 0)
            || position[1] >= MAP.len()
            || position[0] >= MAP[0].len()
            || MAP[position[1]].as_bytes()[position[0]] == b'#'
        {
            return Err(invalid());
        }
        let mut collection = Vec::new();
        for _ in 0..count {
            collection.push(words.monster()?);
        }
        game.state = State {
            position,
            balls,
            encounters,
            wins,
            losses,
            catches,
            starter,
            wild,
            collection,
        };
        let count = words.number::<usize>()?;
        if count > 1_000_000 || game.created.checked_sub(game.retired) != Some(count as u64) {
            return Err(invalid());
        }
        for _ in 0..count {
            let p = Particle {
                pos: [
                    f32::from_bits(words.number()?),
                    f32::from_bits(words.number()?),
                ],
                vel: [
                    f32::from_bits(words.number()?),
                    f32::from_bits(words.number()?),
                ],
                lifetime: words.number()?,
            };
            if p.pos.iter().chain(p.vel.iter()).any(|v| !v.is_finite())
                || p.lifetime == 0
                || p.lifetime > cfg.lifetime
            {
                return Err(invalid());
            }
            game.handles.push(game.alloc.create(p));
        }
        if words.0.next().is_some() {
            return Err(invalid());
        }
        Ok(game)
    }
}
