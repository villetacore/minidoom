//! MiniDoom — упрощённый Doom для крошечных экранов.
//!
//! * Рейкастинг в стиле Wolfenstein 3D / Doom, лабиринт 16x16.
//! * Экран 128x64, 1 бит на пиксель (как у OLED SSD1306 в вейпах).
//!   Буфер уже лежит в «страничном» формате SSD1306 — его можно
//!   слать в дисплей как есть.
//! * `no_std`, без аллокаций, без libm, без FPU. RAM ≈ 1.7 КБ
//!   (кадр 1 КБ + z-буфер 512 Б + состояние игры).
//!
//! Использование на любой платформе:
//! ```ignore
//! let mut game = Game::new();
//! let mut frame = Frame::new();
//! loop {
//!     game.update(read_buttons()); // 30 раз в секунду
//!     game.render(&mut frame);
//!     display.write(&frame.buf);
//! }
//! ```
#![no_std]

pub const W: usize = 128;
pub const H: usize = 64;
/// Рекомендуемая частота вызова `update`.
pub const TICKS_PER_SEC: u32 = 30;

// ---------------------------------------------------------------- кадр

/// Монохромный кадр. Раскладка как у SSD1306: байт = столбец из 8
/// вертикальных пикселей, `buf[(y / 8) * W + x]`, бит `y % 8`.
pub struct Frame {
    pub buf: [u8; W * H / 8],
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Frame {
    pub const fn new() -> Self {
        Self { buf: [0; W * H / 8] }
    }
    pub fn clear(&mut self) {
        self.buf = [0; W * H / 8];
    }
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> bool {
        self.buf[(y >> 3) * W + x] & (1 << (y & 7)) != 0
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, on: bool) {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let i = (y >> 3) * W + x;
        if on {
            self.buf[i] |= 1 << (y & 7);
        } else {
            self.buf[i] &= !(1 << (y & 7));
        }
    }
    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, on: bool) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, on);
            }
        }
    }
    fn invert(&mut self) {
        for b in self.buf.iter_mut() {
            *b = !*b;
        }
    }
}

// ------------------------------------------------------------- ввод

/// Состояние кнопок на текущем тике.
#[derive(Clone, Copy, Default, Debug)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub turn_left: bool,
    pub turn_right: bool,
    pub strafe_left: bool,
    pub strafe_right: bool,
    pub fire: bool,
}

// ------------------------------------------------------------- данные

const MAP_W: usize = 16;
const MAP_H: usize = 16;
static MAP: [&[u8; MAP_W]; MAP_H] = [
    b"################",
    b"#......#.......#",
    b"#..##..#..###..#",
    b"#..#...........#",
    b"#..#..####..#..#",
    b"#.....#..#..#..#",
    b"####..#..#..#..#",
    b"#.....#.....#..#",
    b"#..####..####..#",
    b"#..............#",
    b"#..#..######...#",
    b"#..#.......#...#",
    b"#..####....#...#",
    b"#.......####...#",
    b"#..............#",
    b"################",
];

const ENEMY_SPAWNS: [(f32, f32); 8] = [
    (6.5, 1.5),
    (8.5, 1.5),
    (13.5, 3.5),
    (10.5, 7.5),
    (4.5, 9.5),
    (9.5, 11.5),
    (5.5, 13.5),
    (13.5, 13.5),
];
const N_ENEMIES: usize = ENEMY_SPAWNS.len();

// Спрайт демона 16x16: '#' — белый, 'o' — чёрный, '.' — прозрачный.
static IMP: [&[u8; 16]; 16] = [
    b"...##......##...",
    b"...o##....##o...",
    b"....o######o....",
    b"...o########o...",
    b"...##oo##oo##...",
    b"...##oo##oo##...",
    b"...o########o...",
    b"....#o#o#o#o....",
    b"....o######o....",
    b"..o##########o..",
    b".##o########o##.",
    b".##o########o##.",
    b"....o###o###o...",
    b"....o##o.o##o...",
    b"....o##o.o##o...",
    b"...o###o.o###o..",
];

static GUN: [&[u8; 16]; 12] = [
    b"......o##o......",
    b"......o##o......",
    b"......o##o......",
    b".....o####o.....",
    b".....o####o.....",
    b"....o######o....",
    b"....o##oo##o....",
    b"...o###oo###o...",
    b"...o########o...",
    b"..o##########o..",
    b"..o##########o..",
    b".o############o.",
];

static FLASH: [&[u8; 16]; 7] = [
    b"...#...##...#...",
    b"....#..##..#....",
    b"......####......",
    b"..############..",
    b"......####......",
    b"....#..##..#....",
    b"...#...##...#...",
];

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

// Поворот на фиксированный угол 0.07 рад (cos/sin посчитаны заранее,
// чтобы не тянуть libm).
const ROT_C: f32 = 0.997_551;
const ROT_S: f32 = 0.069_942_85;
const MOVE_SPEED: f32 = 0.08;
const ENEMY_SPEED: f32 = 0.035;
const RADIUS: f32 = 0.2;

// --------------------------------------------------------- математика

#[inline]
fn abs(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// Квадратный корень без libm (быстрый обратный корень + 2 итерации Ньютона).
fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    y *= 1.5 - 0.5 * x * y * y;
    y *= 1.5 - 0.5 * x * y * y;
    x * y
}

fn is_wall(mx: i32, my: i32) -> bool {
    if mx < 0 || my < 0 || mx >= MAP_W as i32 || my >= MAP_H as i32 {
        return true;
    }
    MAP[my as usize][mx as usize] == b'#'
}

/// Свободна ли клетка для тела радиуса RADIUS.
fn free(x: f32, y: f32) -> bool {
    !(is_wall((x - RADIUS) as i32, (y - RADIUS) as i32)
        || is_wall((x + RADIUS) as i32, (y - RADIUS) as i32)
        || is_wall((x - RADIUS) as i32, (y + RADIUS) as i32)
        || is_wall((x + RADIUS) as i32, (y + RADIUS) as i32))
}

fn line_of_sight(x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let steps = (sqrt(dx * dx + dy * dy) * 4.0) as i32 + 1;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        if is_wall((x0 + dx * t) as i32, (y0 + dy * t) as i32) {
            return false;
        }
    }
    true
}

struct RayHit {
    dist: f32,
    side: u8,
    wall_x: f32,
}

// ---------------------------------------------------------------- игра

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Playing,
    Dead,
    Won,
}

#[derive(Clone, Copy)]
struct Enemy {
    x: f32,
    y: f32,
    hp: i8,
    attack_cd: u8,
    hurt: u8,
    moving: bool,
}

pub struct Game {
    px: f32,
    py: f32,
    dx: f32,
    dy: f32,
    plane_x: f32,
    plane_y: f32,
    pub hp: i32,
    pub state: State,
    enemies: [Enemy; N_ENEMIES],
    zbuf: [f32; W],
    tick: u32,
    fire_cd: u8,
    flash: u8,
    hurt: u8,
    bob: u8,
    prev_fire: bool,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        let mut enemies = [Enemy { x: 0.0, y: 0.0, hp: 2, attack_cd: 0, hurt: 0, moving: false }; N_ENEMIES];
        for (e, &(x, y)) in enemies.iter_mut().zip(ENEMY_SPAWNS.iter()) {
            e.x = x;
            e.y = y;
        }
        Self {
            px: 1.5,
            py: 1.5,
            dx: 1.0,
            dy: 0.0,
            // FOV ≈ 66°
            plane_x: 0.0,
            plane_y: 0.66,
            hp: 100,
            state: State::Playing,
            enemies,
            zbuf: [0.0; W],
            tick: 0,
            fire_cd: 0,
            flash: 0,
            hurt: 0,
            bob: 0,
            prev_fire: false,
        }
    }

    pub fn enemies_left(&self) -> usize {
        self.enemies.iter().filter(|e| e.hp > 0).count()
    }

    fn rotate(&mut self, s: f32) {
        let (c, s) = (ROT_C, s);
        let (dx, dy) = (self.dx, self.dy);
        self.dx = dx * c - dy * s;
        self.dy = dx * s + dy * c;
        let (px, py) = (self.plane_x, self.plane_y);
        self.plane_x = px * c - py * s;
        self.plane_y = px * s + py * c;
    }

    fn try_move(&mut self, mx: f32, my: f32) {
        if free(self.px + mx, self.py) {
            self.px += mx;
        }
        if free(self.px, self.py + my) {
            self.py += my;
        }
    }

    /// Один игровой тик (вызывать TICKS_PER_SEC раз в секунду).
    pub fn update(&mut self, inp: Input) {
        let fire_pressed = inp.fire && !self.prev_fire;
        self.prev_fire = inp.fire;
        self.tick = self.tick.wrapping_add(1);

        if self.state != State::Playing {
            if fire_pressed {
                *self = Game::new();
                self.prev_fire = true;
            }
            return;
        }

        // --- игрок
        if inp.turn_left {
            self.rotate(-ROT_S);
        }
        if inp.turn_right {
            self.rotate(ROT_S);
        }
        let mut mx = 0.0;
        let mut my = 0.0;
        if inp.forward {
            mx += self.dx;
            my += self.dy;
        }
        if inp.back {
            mx -= self.dx;
            my -= self.dy;
        }
        if inp.strafe_left {
            mx += self.dy;
            my -= self.dx;
        }
        if inp.strafe_right {
            mx -= self.dy;
            my += self.dx;
        }
        if mx != 0.0 || my != 0.0 {
            self.try_move(mx * MOVE_SPEED, my * MOVE_SPEED);
            self.bob = self.bob.wrapping_add(1);
        }

        // --- стрельба (hitscan по центру экрана)
        self.fire_cd = self.fire_cd.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        self.hurt = self.hurt.saturating_sub(1);
        if fire_pressed && self.fire_cd == 0 {
            self.fire_cd = 8;
            self.flash = 3;
            let wall = self.cast(self.dx, self.dy).dist;
            let mut best: Option<(usize, f32)> = None;
            for (i, e) in self.enemies.iter().enumerate() {
                if e.hp <= 0 {
                    continue;
                }
                if let Some((sx, depth, size)) = self.project(e.x, e.y) {
                    let half = (size as f32 * 0.3).max(2.0);
                    if abs(sx as f32 - (W / 2) as f32) < half
                        && depth < wall
                        && best.map_or(true, |(_, d)| depth < d)
                    {
                        best = Some((i, depth));
                    }
                }
            }
            if let Some((i, depth)) = best {
                let e = &mut self.enemies[i];
                // вблизи урон больше — почти как дробовик
                e.hp -= if depth < 2.0 { 2 } else { 1 };
                e.hurt = 4;
            }
        }

        // --- враги
        for i in 0..N_ENEMIES {
            let mut e = self.enemies[i];
            e.moving = false;
            if e.hp > 0 {
                e.hurt = e.hurt.saturating_sub(1);
                e.attack_cd = e.attack_cd.saturating_sub(1);
                let (vx, vy) = (self.px - e.x, self.py - e.y);
                let d2 = vx * vx + vy * vy;
                if d2 < 64.0 && line_of_sight(e.x, e.y, self.px, self.py) {
                    let d = sqrt(d2);
                    if d > 0.8 {
                        let (sx, sy) = (vx / d * ENEMY_SPEED, vy / d * ENEMY_SPEED);
                        if free(e.x + sx, e.y) {
                            e.x += sx;
                        }
                        if free(e.x, e.y + sy) {
                            e.y += sy;
                        }
                        e.moving = true;
                    } else if e.attack_cd == 0 {
                        e.attack_cd = 20;
                        self.hp -= 8;
                        self.hurt = 4;
                    }
                }
            }
            self.enemies[i] = e;
        }

        if self.hp <= 0 {
            self.hp = 0;
            self.state = State::Dead;
        } else if self.enemies_left() == 0 {
            self.state = State::Won;
        }
    }

    /// DDA-рейкаст до первой стены.
    fn cast(&self, rdx: f32, rdy: f32) -> RayHit {
        let mut mx = self.px as i32;
        let mut my = self.py as i32;
        let ddx = if rdx == 0.0 { 1e30 } else { abs(1.0 / rdx) };
        let ddy = if rdy == 0.0 { 1e30 } else { abs(1.0 / rdy) };
        let (step_x, mut sdx) = if rdx < 0.0 {
            (-1, (self.px - mx as f32) * ddx)
        } else {
            (1, (mx as f32 + 1.0 - self.px) * ddx)
        };
        let (step_y, mut sdy) = if rdy < 0.0 {
            (-1, (self.py - my as f32) * ddy)
        } else {
            (1, (my as f32 + 1.0 - self.py) * ddy)
        };
        let mut side = 0;
        for _ in 0..64 {
            if sdx < sdy {
                sdx += ddx;
                mx += step_x;
                side = 0;
            } else {
                sdy += ddy;
                my += step_y;
                side = 1;
            }
            if is_wall(mx, my) {
                break;
            }
        }
        let dist = if side == 0 { sdx - ddx } else { sdy - ddy }.max(0.05);
        let mut wall_x = if side == 0 { self.py + dist * rdy } else { self.px + dist * rdx };
        wall_x -= wall_x as i32 as f32;
        RayHit { dist, side, wall_x }
    }

    /// Проекция точки мира на экран: (x экрана, глубина, размер).
    fn project(&self, x: f32, y: f32) -> Option<(i32, f32, i32)> {
        let (sx, sy) = (x - self.px, y - self.py);
        let inv = 1.0 / (self.plane_x * self.dy - self.dx * self.plane_y);
        let tx = inv * (self.dy * sx - self.dx * sy);
        let ty = inv * (-self.plane_y * sx + self.plane_x * sy);
        if ty < 0.2 {
            return None;
        }
        let screen_x = ((W / 2) as f32 * (1.0 + tx / ty)) as i32;
        let size = ((H as f32 / ty) as i32).min(H as i32 * 4);
        Some((screen_x, ty, size))
    }

    // ----------------------------------------------------- рендер

    pub fn render(&mut self, f: &mut Frame) {
        f.clear();
        match self.state {
            State::Playing => self.render_world(f),
            State::Dead => {
                self.render_world(f);
                f.invert();
                banner(f, b"YOU DIED");
            }
            State::Won => {
                self.render_world(f);
                banner(f, b"YOU WIN");
            }
        }
    }

    fn render_world(&mut self, f: &mut Frame) {
        let half = (H / 2) as i32;

        // пол: плотность точек растёт к низу экрана
        for y in half..H as i32 {
            let shade = ((y - half) * 5 / half) as u8;
            for x in 0..W as i32 {
                if shade > BAYER[(y & 3) as usize][(x & 3) as usize] {
                    f.set(x, y, true);
                }
            }
        }

        // стены
        for x in 0..W {
            let cam = 2.0 * x as f32 / W as f32 - 1.0;
            let hit = self.cast(self.dx + self.plane_x * cam, self.dy + self.plane_y * cam);
            self.zbuf[x] = hit.dist;
            let lh = ((H as f32 / hit.dist) as i32).min(H as i32 * 8);
            let y0 = half - lh / 2;
            let y1 = half + lh / 2;
            let mut shade = 12.0 - hit.dist * 1.3;
            if hit.side == 1 {
                shade *= 0.6;
            }
            let shade = (shade.max(2.0).min(12.0)) as u8;
            let xi = x as i32;
            // «кирпичная кладка»: 2 ряда блоков, нижний сдвинут на полблока
            let textured = lh > 12;
            for y in y0.max(0)..y1.min(H as i32) {
                let lower = (y - y0) * 2 >= lh;
                let u = hit.wall_x * 2.0 + if lower { 0.5 } else { 0.0 };
                let seam = textured && (u - u as i32 as f32) < 0.08;
                let mortar = textured && (y - y0) == lh / 2;
                let on = !seam && !mortar && shade > BAYER[(y & 3) as usize][x & 3];
                f.set(xi, y, on);
            }
            f.set(xi, y0, true);
            f.set(xi, y1, true);
        }

        // враги: от дальних к ближним
        let mut order = [0usize; N_ENEMIES];
        let mut depth = [0f32; N_ENEMIES];
        for i in 0..N_ENEMIES {
            order[i] = i;
            let e = &self.enemies[i];
            depth[i] = (e.x - self.px) * (e.x - self.px) + (e.y - self.py) * (e.y - self.py);
        }
        for i in 1..N_ENEMIES {
            let mut j = i;
            while j > 0 && depth[order[j - 1]] < depth[order[j]] {
                order.swap(j - 1, j);
                j -= 1;
            }
        }
        for &i in order.iter() {
            let e = self.enemies[i];
            if e.hp <= 0 {
                continue;
            }
            let Some((sx, ty, size)) = self.project(e.x, e.y) else { continue };
            let h = size * 3 / 4;
            let w = h;
            if h < 2 {
                continue;
            }
            let y_end = half + size / 2;
            let y_start = y_end - h;
            let x_start = sx - w / 2;
            let flip = e.moving && (self.tick / 8) % 2 == 1;
            for stripe in x_start.max(0)..(x_start + w).min(W as i32) {
                if ty >= self.zbuf[stripe as usize] {
                    continue;
                }
                let mut tx = ((stripe - x_start) * 16 / w) as usize;
                if flip {
                    tx = 15 - tx;
                }
                for y in y_start.max(0)..y_end.min(H as i32) {
                    let tyi = ((y - y_start) * 16 / h) as usize;
                    match IMP[tyi.min(15)][tx.min(15)] {
                        b'#' => f.set(stripe, y, e.hurt == 0),
                        b'o' => f.set(stripe, y, e.hurt != 0),
                        _ => {}
                    }
                }
            }
        }

        // оружие с покачиванием
        let bob = if self.bob & 8 != 0 { 1 } else { 0 };
        let gx = (W / 2) as i32 - 8;
        let gy = H as i32 - GUN.len() as i32 + bob + if self.flash > 0 { 1 } else { 0 };
        if self.flash > 0 {
            blit(f, &FLASH, gx, gy - FLASH.len() as i32 + 1);
        }
        blit(f, &GUN, gx, gy);

        // прицел
        f.set((W / 2) as i32, half - 1, true);
        f.set((W / 2) as i32, half + 1, true);

        // HUD: здоровье слева, врагов осталось справа
        f.fill_rect(0, 0, 25, 7, false);
        text(f, b"HP", 1, 1, 1);
        number(f, self.hp as u32, 10, 1);
        f.fill_rect(W as i32 - 21, 0, 21, 7, false);
        text(f, b"K", W as i32 - 20, 1, 1);
        number(f, self.enemies_left() as u32, W as i32 - 14, 1);

        // получили урон — мигающая рамка
        if self.hurt > 0 {
            for x in 0..W as i32 {
                f.set(x, 0, true);
                f.set(x, H as i32 - 1, true);
            }
            for y in 0..H as i32 {
                f.set(0, y, true);
                f.set(W as i32 - 1, y, true);
            }
        }
    }
}

fn blit<const N: usize>(f: &mut Frame, img: &[&[u8; N]], x0: i32, y0: i32) {
    for (dy, row) in img.iter().enumerate() {
        for (dx, &c) in row.iter().enumerate() {
            match c {
                b'#' => f.set(x0 + dx as i32, y0 + dy as i32, true),
                b'o' => f.set(x0 + dx as i32, y0 + dy as i32, false),
                _ => {}
            }
        }
    }
}

// ------------------------------------------------------ шрифт 3x5

fn glyph(c: u8) -> [u8; 5] {
    match c {
        b'0' | b'O' => [7, 5, 5, 5, 7],
        b'1' => [2, 6, 2, 2, 7],
        b'2' => [7, 1, 7, 4, 7],
        b'3' => [7, 1, 7, 1, 7],
        b'4' => [5, 5, 7, 1, 1],
        b'5' | b'S' => [7, 4, 7, 1, 7],
        b'6' => [7, 4, 7, 5, 7],
        b'7' => [7, 1, 1, 2, 2],
        b'8' => [7, 5, 7, 5, 7],
        b'9' => [7, 5, 7, 1, 7],
        b'A' => [2, 5, 7, 5, 5],
        b'D' => [6, 5, 5, 5, 6],
        b'E' => [7, 4, 6, 4, 7],
        b'F' => [7, 4, 6, 4, 4],
        b'H' => [5, 5, 7, 5, 5],
        b'I' => [7, 2, 2, 2, 7],
        b'K' => [5, 6, 4, 6, 5],
        b'N' => [5, 7, 7, 5, 5],
        b'P' => [7, 5, 7, 4, 4],
        b'R' => [6, 5, 6, 5, 5],
        b'T' => [7, 2, 2, 2, 2],
        b'U' => [5, 5, 5, 5, 7],
        b'W' => [5, 5, 7, 7, 5],
        b'Y' => [5, 5, 2, 2, 2],
        _ => [0; 5],
    }
}

fn text(f: &mut Frame, s: &[u8], x: i32, y: i32, scale: i32) {
    for (i, &c) in s.iter().enumerate() {
        let g = glyph(c);
        let cx = x + i as i32 * 4 * scale;
        for (row, bits) in g.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    f.fill_rect(cx + col * scale, y + row as i32 * scale, scale, scale, true);
                }
            }
        }
    }
}

fn number(f: &mut Frame, mut n: u32, x: i32, y: i32) {
    let mut digits = [b'0'; 3];
    for d in digits.iter_mut().rev() {
        *d = b'0' + (n % 10) as u8;
        n /= 10;
    }
    text(f, &digits, x, y, 1);
}

fn banner(f: &mut Frame, msg: &[u8]) {
    let scale = 2;
    let w = msg.len() as i32 * 4 * scale;
    let x = (W as i32 - w) / 2;
    f.fill_rect(x - 3, 14, w + 5, 32, false);
    text(f, msg, x, 17, scale);
    let sub = b"FIRE TO RESTART";
    let sw = sub.len() as i32 * 4;
    text(f, sub, (W as i32 - sw) / 2, 36, 1);
}

// ---------------------------------------------------------- тесты

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_are_free() {
        assert!(free(1.5, 1.5));
        for &(x, y) in ENEMY_SPAWNS.iter() {
            assert!(free(x, y), "враг в стене: {x},{y}");
        }
    }

    #[test]
    fn sqrt_ok() {
        for &v in &[0.25f32, 1.0, 2.0, 9.0, 50.0] {
            assert!(abs(sqrt(v) * sqrt(v) - v) < 1e-3 * v.max(1.0));
        }
    }

    #[test]
    fn long_random_session_does_not_panic() {
        let mut g = Game::new();
        let mut f = Frame::new();
        let mut r: u32 = 12345;
        for _ in 0..20_000 {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            let inp = Input {
                forward: r & 1 != 0,
                back: r & 2 != 0 && r & 64 == 0,
                turn_left: r & 4 != 0,
                turn_right: r & 8 != 0,
                strafe_left: r & 16 != 0,
                strafe_right: r & 32 != 0,
                fire: r & 128 != 0,
            };
            g.update(inp);
            g.render(&mut f);
        }
    }
}
