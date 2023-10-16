#![allow(unused_imports)]

#![no_main]
#![no_std]

// use defmt_rtt as _;
use panic_halt as _;

use rtic::app;

#[app(device = microbit::pac, peripherals = true)]
mod app {

    use microbit::{
        board::Board,
        display::nonblocking::{Display, GreyscaleImage},
        hal::{
            prelude::*,
            clocks::Clocks,
            rtc::{Rtc, RtcInterrupt}, Timer, timer::Periodic,
            timer::Instance,
        },
        pac, Peripherals,
    };
    use rtt_target::{rprintln, rtt_init_print};

    // fn heart_image(inner_brightness: u8) -> GreyscaleImage {
    //     let b = inner_brightness;
    //     GreyscaleImage::new(&[
    //         [0, 7, 0, 7, 0],
    //         [7, b, 7, b, 7],
    //         [7, b, b, b, 7],
    //         [0, 7, b, 7, 0],
    //         [0, 0, 7, 0, 0],
    //     ])
    // }

    // #[shared]
    // struct Shared {
    //     display: Display<pac::TIMER1>,
    // }

    // #[local]
    // struct Local {
    //     anim_timer: Rtc<pac::RTC0>,
    // }


    #[shared]
    struct Shared {
    }

    #[local]
    struct Local {
        timer0: Timer<pac::TIMER0, Periodic>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        rtt_init_print!();
        let board = Board::new(cx.device, cx.core);
        
        let mut timer0 = Timer::periodic(board.TIMER0);
        timer0.start(1_000_000u32);
        timer0.enable_interrupt();

        (Shared {} , Local { timer0 }, init::Monotonics())
    }

    #[task(binds = TIMER0, local = [timer0])]
    fn timer0(cx: timer0::Context) {
        rprintln!("timer 0 ticked !");
        let _ = cx.local.timer0.wait(); // consume the event
    }

    // #[init]
    // fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
    //     let board = Board::new(cx.device, cx.core);

    //     // Starting the low-frequency clock (needed for RTC to work)
    //     Clocks::new(board.CLOCK).start_lfclk();

    //     // RTC at 16Hz (32_768 / (2047 + 1))
    //     // 16Hz; 62.5ms period
    //     let mut rtc0 = Rtc::new(board.RTC0, 2047).unwrap();
    //     rtc0.enable_event(RtcInterrupt::Tick);
    //     rtc0.enable_interrupt(RtcInterrupt::Tick, None);
    //     rtc0.enable_counter();

    //     let display = Display::new(board.TIMER1, board.display_pins);
    //     (
    //         Shared { display },
    //         Local { anim_timer: rtc0 },
    //         init::Monotonics(),
    //     )
    // }

    // #[task(binds = TIMER1, priority = 2, shared = [display])]
    // fn timer1(mut cx: timer1::Context) {
    //     cx.shared
    //         .display
    //         .lock(|display| display.handle_display_event());
    // }

    // #[task(binds = RTC0, priority = 1, shared = [display],
    //        local = [anim_timer, step: u8 = 0])]
    // fn rtc0(cx: rtc0::Context) {
    //     let mut shared = cx.shared;
    //     let local = cx.local;

    //     local.anim_timer.reset_event(RtcInterrupt::Tick);

    //     let inner_brightness = match *local.step {
    //         0..=8 => 9 - *local.step,
    //         9..=12 => 0,
    //         _ => unreachable!(),
    //     };

    //     shared.display.lock(|display| {
    //         display.show(&heart_image(inner_brightness));
    //     });

    //     *local.step += 1;
    //     if *local.step == 13 {
    //         *local.step = 0
    //     };
    // }
}
