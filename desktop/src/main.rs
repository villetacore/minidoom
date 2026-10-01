//! Запуск MiniDoom на ПК в окне, которое выглядит как TFT 160x80 от SMOK Knight 80
//! (кадр 128x64 растянут в 1.25 раза — как на железе).
//!
//! Управление:
//!   W / ↑  — вперёд        S / ↓  — назад
//!   ← / →  — поворот       A / D  — шаг вбок
//!   Пробел / Ctrl — выстрел          Esc — выход
//!   V — режим «3 кнопки вейпа» (как будет на железе)
use minidoom_core::{Frame, Game, Input, H, TICKS_PER_SEC, W};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

const LCD_W: usize = 160;
const LCD_H: usize = 80;
const ON: u32 = 0x00_FF_8C_14; // те же цвета, что COLOR_ON/COLOR_OFF в прошивке
const OFF: u32 = 0x00_00_00_00;

fn main() {
    let mut window = Window::new(
        "MiniDoom Knight 80 (160x80) — Esc выход, V режим вейпа",
        LCD_W,
        LCD_H,
        WindowOptions { scale: Scale::X8, ..WindowOptions::default() },
    )
    .expect("не удалось открыть окно");
    window.set_target_fps(TICKS_PER_SEC as usize);

    let mut game = Game::new();
    let mut frame = Frame::new();
    let mut pixels = vec![0u32; LCD_W * LCD_H];
    let mut vape_mode = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::V, KeyRepeat::No) {
            vape_mode = !vape_mode;
            println!("режим 3 кнопок вейпа: {}", if vape_mode { "вкл" } else { "выкл" });
        }
        let k = |key| window.is_key_down(key);

        let input = if vape_mode {
            // Эмуляция 3 кнопок: «+» (→), «−» (←), «огонь» (пробел).
            // См. vape_buttons_to_input в README — та же логика на железе.
            vape_buttons_to_input(k(Key::Left), k(Key::Right), k(Key::Space))
        } else {
            Input {
                forward: k(Key::W) || k(Key::Up),
                back: k(Key::S) || k(Key::Down),
                turn_left: k(Key::Left),
                turn_right: k(Key::Right),
                strafe_left: k(Key::A),
                strafe_right: k(Key::D),
                fire: k(Key::Space) || k(Key::LeftCtrl) || k(Key::RightCtrl),
            }
        };

        game.update(input);
        game.render(&mut frame);

        for y in 0..LCD_H {
            for x in 0..LCD_W {
                let on = frame.get(x * W / LCD_W, y * H / LCD_H);
                pixels[y * LCD_W + x] = if on { ON } else { OFF };
            }
        }
        window.update_with_buffer(&pixels, LCD_W, LCD_H).unwrap();
    }
}

/// Схема для 3 кнопок: «−» = поворот влево, «+» = поворот вправо,
/// обе сразу = идти вперёд, «огонь» = выстрел.
fn vape_buttons_to_input(minus: bool, plus: bool, fire: bool) -> Input {
    Input {
        forward: minus && plus,
        turn_left: minus && !plus,
        turn_right: plus && !minus,
        fire,
        ..Input::default()
    }
}
