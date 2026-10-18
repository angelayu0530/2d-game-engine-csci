use super::{BALL_RADIUS, HEIGHT, LEFT_FACE, PADDLE_HALF_H, Pong, RIGHT_FACE, Side, WIDTH};
use crate::{alloc::Allocator, canvas::Canvas};

pub const BACKGROUND: u32 = 0x10_14_18;
pub const FOREGROUND: u32 = 0xE8_E8_E8;
const NET: u32 = 0x40_44_48;
const SPARK: [u32; 3] = [0xFF, 0xB0, 0x40];
const PADDLE_W: f32 = 12.0;
const SCORE_CELL: f32 = 8.0;

pub fn draw<A: Allocator>(game: &Pong<A>, canvas: &mut Canvas) {
    canvas.clear(BACKGROUND);
    for y in (0..HEIGHT as u32).step_by(32) {
        canvas.fill_rect(WIDTH / 2.0 - 2.0, y as f32 + 8.0, 4.0, 16.0, NET);
    }

    let [left, right] = game.score();
    let left_width = (left.to_string().len() * 4 - 1) as f32 * SCORE_CELL;
    canvas.number(
        left,
        WIDTH / 2.0 - 48.0 - left_width,
        32.0,
        SCORE_CELL,
        FOREGROUND,
    );
    canvas.number(right, WIDTH / 2.0 + 48.0, 32.0, SCORE_CELL, FOREGROUND);

    let max_life = game.config().particle_lifetime.max(1) as f32;
    for particle in game.particles() {
        let color = fade(particle.lifetime as f32 / max_life);
        canvas.fill_rect(
            particle.pos[0] - 1.5,
            particle.pos[1] - 1.5,
            3.0,
            3.0,
            color,
        );
    }

    let paddle_h = PADDLE_HALF_H * 2.0;
    let left_y = game.paddle_y(Side::Left) - PADDLE_HALF_H;
    let right_y = game.paddle_y(Side::Right) - PADDLE_HALF_H;
    canvas.fill_rect(
        LEFT_FACE - BALL_RADIUS - PADDLE_W,
        left_y,
        PADDLE_W,
        paddle_h,
        FOREGROUND,
    );
    canvas.fill_rect(
        RIGHT_FACE + BALL_RADIUS,
        right_y,
        PADDLE_W,
        paddle_h,
        FOREGROUND,
    );

    let ball = game.ball().pos;
    let size = BALL_RADIUS * 2.0;
    canvas.fill_rect(
        ball[0] - BALL_RADIUS,
        ball[1] - BALL_RADIUS,
        size,
        size,
        FOREGROUND,
    );
}

fn fade(t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |shift: u32, spark: u32| {
        let bg = (BACKGROUND >> shift & 0xFF) as f32;
        ((bg + (spark as f32 - bg) * t) as u32) << shift
    };
    channel(16, SPARK[0]) | channel(8, SPARK[1]) | channel(0, SPARK[2])
}

#[cfg(test)]
mod tests {
    use super::{BACKGROUND, FOREGROUND, PADDLE_W, SPARK, draw, fade};
    use crate::{
        alloc::pool::PoolAllocator,
        canvas::Canvas,
        pong::{BALL_RADIUS, Ball, HEIGHT, LEFT_FACE, Pong, PongConfig, RIGHT_FACE, Side, WIDTH},
    };

    fn at(canvas: &Canvas, x: f32, y: f32) -> u32 {
        canvas.pixels()[y as usize * canvas.width() + x as usize]
    }

    #[test]
    fn draws_ball_and_paddles_where_the_sim_has_them() {
        let mut game = Pong::<PoolAllocator>::new(&PongConfig {
            burst: 0,
            ..PongConfig::default()
        });
        game.set_ball(Ball {
            pos: [300.0, 200.0],
            vel: [0.0, 0.0],
        });
        game.set_paddle_y(Side::Left, 100.0);
        game.set_paddle_y(Side::Right, 600.0);
        let mut canvas = Canvas::new(WIDTH as usize, HEIGHT as usize);
        draw(&game, &mut canvas);

        assert_eq!(at(&canvas, 300.0, 200.0), FOREGROUND);
        assert_eq!(at(&canvas, 300.0 + BALL_RADIUS + 1.0, 200.0), BACKGROUND);

        let left_x = LEFT_FACE - BALL_RADIUS - PADDLE_W / 2.0;
        let right_x = RIGHT_FACE + BALL_RADIUS + PADDLE_W / 2.0;
        assert_eq!(at(&canvas, left_x, 100.0), FOREGROUND);
        assert_eq!(at(&canvas, left_x, 600.0), BACKGROUND);
        assert_eq!(at(&canvas, right_x, 600.0), FOREGROUND);
        assert_eq!(at(&canvas, right_x, 100.0), BACKGROUND);
    }

    #[test]
    fn particles_fade_from_spark_to_background() {
        let spark = SPARK[0] << 16 | SPARK[1] << 8 | SPARK[2];
        assert_eq!(fade(1.0), spark);
        assert_eq!(fade(0.0), BACKGROUND);
        assert_eq!(fade(7.0), spark);
    }
}
