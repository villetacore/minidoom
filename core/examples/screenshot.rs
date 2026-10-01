//! Рендер без окна: прогоняет несколько тиков и сохраняет кадры в PBM
//! (открывается в GIMP / IrfanView / любом конвертере).
//! Запуск: cargo run -p minidoom-core --example screenshot
use minidoom_core::{Frame, Game, Input, H, W};
use std::io::Write;

fn save(frame: &Frame, path: &str) {
    let mut out = std::fs::File::create(path).unwrap();
    write!(out, "P1\n{W} {H}\n").unwrap();
    for y in 0..H {
        for x in 0..W {
            // в PBM 1 = чёрный, а у нас 1 = светящийся пиксель
            out.write_all(if frame.get(x, y) { b"0 " } else { b"1 " }).unwrap();
        }
        out.write_all(b"\n").unwrap();
    }
    println!("сохранено: {path}");
}

fn main() {
    let mut g = Game::new();
    let mut f = Frame::new();
    g.render(&mut f);
    save(&f, "shot_start.pbm");

    // демон впереди идёт на нас — ждём и стреляем
    for i in 0..45 {
        g.update(Input { fire: i == 44, ..Default::default() });
    }
    g.render(&mut f);
    save(&f, "shot_fire.pbm");

    // стоим и получаем урон
    for _ in 0..60 {
        g.update(Input::default());
    }
    g.render(&mut f);
    save(&f, "shot_close.pbm");
}
