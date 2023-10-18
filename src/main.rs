#![allow(unused_imports)]
#![no_main]
#![no_std]

// use defmt_rtt as _;
use panic_halt as _;

use rtic::app;

#[app(device = microbit::pac, peripherals = true)]
mod app {
    use super::*;

    use microbit::{
        board::{Board, Buttons},
        display::nonblocking::{Display, GreyscaleImage},
        gpio,
        hal::{
            clocks::Clocks,
            gpiote::{Gpiote, GpioteChannel},
            prelude::*,
            rtc::{Rtc, RtcInterrupt},
            timer::Instance,
            timer::Periodic,
            Timer,
        },
        pac, Peripherals,
    };
    use rtt_target::{rdbg, rprintln, rtt_init_print};
    use void::{ResultVoidExt, Void};

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        display_timer: Timer<pac::TIMER0, Periodic>,
        debounce_timer: Timer<pac::TIMER1, Periodic>,
        pins: gpio::DisplayPins,
        buttons: Buttons,
        debouncers: [Debouncer; 2]
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        rtt_init_print!();
        let mut board = Board::new(cx.device, cx.core);

        let clocks = Clocks::new(board.CLOCK);
        clocks.enable_ext_hfosc();

        toggle(&mut board.display_pins.row1);
        toggle(&mut board.display_pins.col1);

        // LED display timer
        let mut timer0 = Timer::periodic(board.TIMER0);
        timer0.start(1_000_000u32);
        timer0.enable_interrupt();

        // button debounce timer
        let mut timer1 = Timer::periodic(board.TIMER1);
        timer1.start(5_000u32);
        timer1.enable_interrupt();
        let debouncers = [
            Debouncer::new(2, 10),
            Debouncer::new(2, 10)
        ];

        (
            Shared {},
            Local {
                display_timer: timer0,
                debounce_timer: timer1,
                pins: board.display_pins,
                buttons: board.buttons,
                debouncers: debouncers,
            },
            init::Monotonics(),
        )
    }

    #[task(binds = TIMER0, local = [display_timer, pins])]
    fn handle_display_timer(cx: handle_display_timer::Context) {
        let _ = cx.local.display_timer.wait(); // consume the event
        let pins = cx.local.pins;
        toggle(&mut pins.col1);
        rprintln!("timer 0 ticked !");
    }

    #[task(binds = TIMER1, local = [debounce_timer, buttons, debouncers])]
    fn handle_debounce_timer(cx: handle_debounce_timer::Context) {
        let _ = cx.local.debounce_timer.wait(); // consume the event
        let result = [
            read_debounced_button(&cx.local.buttons.button_a, &mut cx.local.debouncers[0]),
            read_debounced_button(&cx.local.buttons.button_b, &mut cx.local.debouncers[1]),
        ];

        if result[0] == None && result[1] == None {
            return;
        }
        rdbg!(result);
    }

    fn read_debounced_button(btn: &dyn InputPin<Error = Void>, debouncer: &mut Debouncer) -> Option<BtnState> {
        let raw_state = if btn.is_high().void_unwrap() { BtnState::NotPressed } else { BtnState::Pressed };
        return debouncer.update(raw_state);
    }
}

use microbit::hal::prelude::*;
use void::{ResultVoidExt, Void};

fn toggle(pin: &mut dyn StatefulOutputPin<Error = Void>) {
    if pin.is_set_high().void_unwrap() {
        pin.set_low().void_unwrap();
    } else {
        pin.set_high().void_unwrap();
    }
}

#[derive(PartialEq)]
pub struct Debouncer {
    press_ticks: usize,
    release_ticks: usize,
    btn_state: BtnState,
    count: usize,
}

#[derive(PartialEq, Copy, Clone, Debug)]
enum BtnState {
    Pressed,
    NotPressed,
}

impl Debouncer {
    const fn new(press_ticks: usize, release_ticks: usize) -> Self {
        Debouncer {
            press_ticks,
            release_ticks,
            btn_state: BtnState::NotPressed,
            count: 0,
        }
    }

    fn update(&mut self, raw_state: BtnState) -> Option<BtnState> {
        if self.btn_state == raw_state {
            self.count = 0;
            return None;
        }

        let target_ticks = if raw_state == BtnState::Pressed {
            self.press_ticks
        } else {
            self.release_ticks
        };

        self.count += 1;
        if self.count >= target_ticks {
            self.count = 0;
            self.btn_state = raw_state;
            return Some(self.btn_state);
        } else {
            return None;
        }
    }
}

/*
use rtt_target::{rprintln, rdbg};
fn test_debouncer() {
    rprintln!("test_debouncer");
    let mut d = Debouncer::new(2, 4);

    rdbg!(d.update(BtnState::Pressed));
    rdbg!(d.update(BtnState::Pressed));
    rdbg!(d.update(BtnState::Pressed));
    rprintln!("");

    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rprintln!("");

    rdbg!(d.update(BtnState::Pressed));
    rdbg!(d.update(BtnState::Pressed));
    rdbg!(d.update(BtnState::Pressed));
    rprintln!("");

    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rdbg!(d.update(BtnState::NotPressed));
    rprintln!("");

}
*/
