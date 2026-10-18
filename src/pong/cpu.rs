use super::{BALL_RADIUS, Ball, HEIGHT, PADDLE_HALF_H, Side};
use crate::particle::{signed_unit, splitmix64};

const REACH: f32 = PADDLE_HALF_H + BALL_RADIUS;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuConfig {
    pub speed: f32,
    pub aim: f32,
    pub deadband: f32,
    pub variance: f32,
    pub seed: u64,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            speed: 0.75,
            aim: 0.7,
            deadband: 16.0,
            variance: 1.0,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cpu {
    side: Side,
    cfg: CpuConfig,
    rng: u64,
    approaching: bool,
    error: f32,
}

impl Cpu {
    pub fn new(side: Side, cfg: CpuConfig) -> Self {
        let salt = match side {
            Side::Left => 0x4C45_4654,
            Side::Right => 0x5249_4748,
        };
        Self {
            side,
            cfg,
            rng: splitmix64(cfg.seed ^ salt),
            approaching: false,
            error: 0.0,
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn input(&mut self, paddle_y: f32, ball: Ball) -> f32 {
        let target = self.target(ball);
        ((target - paddle_y) / self.cfg.deadband).clamp(-1.0, 1.0) * self.cfg.speed
    }

    pub fn target(&mut self, ball: Ball) -> f32 {
        let approaching = match self.side {
            Side::Left => ball.vel[0] < 0.0,
            Side::Right => ball.vel[0] > 0.0,
        };
        if approaching && !self.approaching {
            self.rng = splitmix64(self.rng);
            self.error = signed_unit(self.rng as u32) * self.cfg.variance * REACH;
        }
        self.approaching = approaching;
        if !approaching {
            return HEIGHT * 0.5;
        }
        let steer = if ball.vel[1] < 0.0 { -1.0 } else { 1.0 };
        ball.pos[1] - steer * self.cfg.aim * REACH + self.error
    }
}

#[cfg(test)]
mod tests {
    use super::{Cpu, CpuConfig, REACH};
    use crate::pong::{Ball, HEIGHT, Side};

    fn ball(x_dir: f32, y: f32, vy: f32) -> Ball {
        Ball {
            pos: [500.0, y],
            vel: [x_dir * 300.0, vy],
        }
    }

    fn exact() -> CpuConfig {
        CpuConfig {
            variance: 0.0,
            ..CpuConfig::default()
        }
    }

    #[test]
    fn recentres_while_the_ball_recedes() {
        let mut left = Cpu::new(Side::Left, exact());
        let mut right = Cpu::new(Side::Right, exact());
        assert_eq!(left.target(ball(1.0, 100.0, 50.0)), HEIGHT * 0.5);
        assert_eq!(right.target(ball(-1.0, 100.0, 50.0)), HEIGHT * 0.5);
    }

    #[test]
    fn aims_off_centre_with_the_ball_vertical_direction() {
        let cfg = exact();
        let mut right = Cpu::new(Side::Right, cfg);
        let offset = cfg.aim * REACH;
        assert_eq!(right.target(ball(1.0, 300.0, 80.0)), 300.0 - offset);
        assert_eq!(right.target(ball(1.0, 300.0, -80.0)), 300.0 + offset);
    }

    #[test]
    fn input_saturates_far_away_and_eases_inside_the_deadband() {
        let cfg = exact();
        let mut right = Cpu::new(Side::Right, cfg);
        let target = right.target(ball(1.0, 400.0, 10.0));
        assert_eq!(
            right.input(target - 200.0, ball(1.0, 400.0, 10.0)),
            cfg.speed
        );
        assert_eq!(
            right.input(target + 200.0, ball(1.0, 400.0, 10.0)),
            -cfg.speed
        );
        let near = right.input(target - cfg.deadband / 2.0, ball(1.0, 400.0, 10.0));
        assert_eq!(near, cfg.speed / 2.0);
    }

    #[test]
    fn variance_rolls_one_bounded_error_per_approach() {
        let cfg = CpuConfig {
            variance: 0.5,
            ..CpuConfig::default()
        };
        let mut cpu = Cpu::new(Side::Right, cfg);
        let mut errors = Vec::new();
        for _ in 0..64 {
            cpu.target(ball(-1.0, 300.0, 80.0));
            let base = 300.0 - cfg.aim * REACH;
            let first = cpu.target(ball(1.0, 300.0, 80.0)) - base;
            let later = cpu.target(ball(1.0, 300.0, 80.0)) - base;
            assert_eq!(first, later);
            assert!(first.abs() <= cfg.variance * REACH, "{first}");
            errors.push(first);
        }
        errors.sort_by(f32::total_cmp);
        errors.dedup();
        assert!(errors.len() > 32, "{errors:?}");
        assert!(errors[0] < 0.0 && errors[errors.len() - 1] > 0.0);
    }

    #[test]
    fn same_seed_same_errors_different_seed_different_errors() {
        let roll = |seed| {
            let mut cpu = Cpu::new(
                Side::Left,
                CpuConfig {
                    seed,
                    ..CpuConfig::default()
                },
            );
            (0..8)
                .map(|_| {
                    cpu.target(ball(1.0, 300.0, 0.0));
                    cpu.target(ball(-1.0, 300.0, 0.0))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(roll(7), roll(7));
        assert_ne!(roll(7), roll(8));
    }
}
