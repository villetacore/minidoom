//! MiniDoom на Raspberry Pi Pico (RP2040) с экраном от SMOK Knight 80:
//! цветной TFT 0.96" 160x80, контроллер ST7735S, 4-проводной SPI.
//!
//! Кадр движка 128x64 растягивается ровно в 1.25 раза -> 160x80 (весь экран).
//!
//! Подключение шлейфа экрана (названия контактов как обычно пишут на
//! шлейфах 0.96" TFT: SCL/SDA — это SPI, не I2C!):
//!   GND  -> GND               VCC -> 3V3 (пин 36)
//!   SCL  -> GP18 (пин 24)     SPI0 SCK
//!   SDA  -> GP19 (пин 25)     SPI0 MOSI
//!   CS   -> GP17 (пин 22)
//!   DC/RS-> GP16 (пин 21)
//!   RES  -> GP20 (пин 26)
//!   BLK/LED(A) -> GP21 (пин 27) — подсветка (или через 10-47 Ом на 3V3)
//!
//! Кнопки Knight 80 (замыкают пин на GND, подтяжка внутренняя):
//!   GP10 — «−» / влево         GP11 — «+» / вправо
//!   GP12 — «огонь» (выстрел)
//!   GP13 — вперёд, GP14 — назад (необязательные, отдельные кнопки)
//!   Схема «3 кнопки»: − и + вместе = идти вперёд.
#![no_std]
#![no_main]

use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiBus;
use minidoom_core::{Frame, Game, Input, H, TICKS_PER_SEC, W};
use panic_halt as _;
use rp_pico::entry;
use rp_pico::hal::{self, fugit::RateExtU32, gpio, pac, Clock};

// ------------------------------------------------ настройки экрана

const LCD_W: usize = 160;
const LCD_H: usize = 80;

/// Смещение видимой области в памяти ST7735S. У 0.96" панелей 80x160
/// в горизонтальной ориентации обычно (1, 26). Если по краю мусор или
/// картинка обрезана — попробуйте (0, 24) или (26, 1).
const X_OFF: u16 = 1;
const Y_OFF: u16 = 26;

/// Ориентация (MADCTL): MY=0x80 MX=0x40 MV=0x20 BGR=0x08.
/// 0x68 — горизонтально. Вверх ногами — 0xA8. Зеркально — поменяйте 0x40/0x80.
const MADCTL: u8 = 0x68;

/// IPS-панели (как в большинстве вейпов) показывают негатив без инверсии.
/// Если цвета вывернуты (фон белый) — поставьте false.
const INVERT: bool = true;

/// Цвета в RGB565. Кадр монохромный, выбираем «горящий» и фоновый цвет.
const COLOR_ON: u16 = rgb565(255, 140, 20); // оранжевый, «адский»
const COLOR_OFF: u16 = rgb565(0, 0, 0);

const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
}

// ------------------------------------------------ мини-драйвер ST7735S

struct Lcd<S, DC, CS> {
    spi: S,
    dc: DC,
    cs: CS,
}

impl<S: SpiBus, DC: OutputPin, CS: OutputPin> Lcd<S, DC, CS> {
    fn cmd(&mut self, c: u8, data: &[u8]) {
        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        let _ = self.spi.write(&[c]);
        let _ = self.spi.flush(); // DC нельзя дёргать, пока байт не ушёл
        let _ = self.dc.set_high();
        if !data.is_empty() {
            let _ = self.spi.write(data);
            let _ = self.spi.flush();
        }
        let _ = self.cs.set_high();
    }

    fn init(&mut self, delay_ms: &mut impl FnMut(u32)) {
        self.cmd(0x01, &[]); // SWRESET
        delay_ms(150);
        self.cmd(0x11, &[]); // SLPOUT
        delay_ms(120);
        self.cmd(0xB1, &[0x01, 0x2C, 0x2D]); // частота кадров
        self.cmd(0xB2, &[0x01, 0x2C, 0x2D]);
        self.cmd(0xB3, &[0x01, 0x2C, 0x2D, 0x01, 0x2C, 0x2D]);
        self.cmd(0xB4, &[0x07]); // инверсия строк
        self.cmd(0xC0, &[0xA2, 0x02, 0x84]); // питание
        self.cmd(0xC1, &[0xC5]);
        self.cmd(0xC2, &[0x0A, 0x00]);
        self.cmd(0xC3, &[0x8A, 0x2A]);
        self.cmd(0xC4, &[0x8A, 0xEE]);
        self.cmd(0xC5, &[0x0E]); // VCOM
        self.cmd(if INVERT { 0x21 } else { 0x20 }, &[]);
        self.cmd(0x36, &[MADCTL]);
        self.cmd(0x3A, &[0x05]); // 16 бит на пиксель
        self.cmd(0x13, &[]); // NORON
        delay_ms(10);
        self.cmd(0x29, &[]); // DISPON
        delay_ms(20);
    }

    /// Растягивает кадр 128x64 в 160x80 (x1.25, ближайший сосед) и шлёт.
    fn flush(&mut self, frame: &Frame) {
        let (x0, x1) = (X_OFF, X_OFF + LCD_W as u16 - 1);
        let (y0, y1) = (Y_OFF, Y_OFF + LCD_H as u16 - 1);
        self.cmd(0x2A, &[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8]);
        self.cmd(0x2B, &[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8]);

        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        let _ = self.spi.write(&[0x2C]); // RAMWR
        let _ = self.spi.flush();
        let _ = self.dc.set_high();

        let mut line = [0u8; LCD_W * 2];
        for y in 0..LCD_H {
            let sy = y * H / LCD_H;
            for x in 0..LCD_W {
                let c = if frame.get(x * W / LCD_W, sy) { COLOR_ON } else { COLOR_OFF };
                line[2 * x] = (c >> 8) as u8;
                line[2 * x + 1] = c as u8;
            }
            let _ = self.spi.write(&line);
        }
        let _ = self.spi.flush();
        let _ = self.cs.set_high();
    }
}

// ------------------------------------------------------------ main

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let clocks = hal::clocks::init_clocks_and_plls(
        rp_pico::XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let sio = hal::Sio::new(pac.SIO);
    let pins = rp_pico::Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);
    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);
    let mut delay_ms = |ms: u32| {
        let end = timer.get_counter().ticks() + ms as u64 * 1000;
        while timer.get_counter().ticks() < end {}
    };

    // SPI0: SCK=GP18, MOSI=GP19. 32 МГц — кадр уходит за ~7 мс.
    // Если на экране полосы/мусор — снизьте до 16.MHz() или 8.MHz().
    let sck = pins.gpio18.into_function::<gpio::FunctionSpi>();
    let mosi = pins.gpio19.into_function::<gpio::FunctionSpi>();
    let spi = hal::Spi::<_, _, _, 8>::new(pac.SPI0, (mosi, sck)).init(
        &mut pac.RESETS,
        clocks.peripheral_clock.freq(),
        32.MHz(),
        embedded_hal::spi::MODE_0,
    );

    let mut cs = pins.gpio17.into_push_pull_output();
    let dc = pins.gpio16.into_push_pull_output();
    let mut rst = pins.gpio20.into_push_pull_output();
    let mut backlight = pins.gpio21.into_push_pull_output();
    let _ = cs.set_high();

    // аппаратный сброс экрана
    let _ = rst.set_high();
    delay_ms(5);
    let _ = rst.set_low();
    delay_ms(20);
    let _ = rst.set_high();
    delay_ms(150);

    let mut lcd = Lcd { spi, dc, cs };
    lcd.init(&mut delay_ms);
    let _ = backlight.set_high();

    let mut btn_minus = pins.gpio10.into_pull_up_input();
    let mut btn_plus = pins.gpio11.into_pull_up_input();
    let mut btn_fire = pins.gpio12.into_pull_up_input();
    let mut btn_fwd = pins.gpio13.into_pull_up_input();
    let mut btn_back = pins.gpio14.into_pull_up_input();

    let mut game = Game::new();
    let mut frame = Frame::new();
    let tick_us: u64 = 1_000_000 / TICKS_PER_SEC as u64;
    let mut next = timer.get_counter().ticks();

    loop {
        // кнопка нажата = пин прижат к GND
        let minus = btn_minus.is_low().unwrap_or(false);
        let plus = btn_plus.is_low().unwrap_or(false);
        let fire = btn_fire.is_low().unwrap_or(false);
        let fwd = btn_fwd.is_low().unwrap_or(false);
        let back = btn_back.is_low().unwrap_or(false);

        game.update(Input {
            forward: fwd || (minus && plus),
            back,
            turn_left: minus && !plus,
            turn_right: plus && !minus,
            fire,
            ..Input::default()
        });
        game.render(&mut frame);
        lcd.flush(&frame);

        // держим 30 тиков в секунду
        next += tick_us;
        while timer.get_counter().ticks() < next {}
    }
}
