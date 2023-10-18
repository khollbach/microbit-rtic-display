#![allow(unused_imports)]
#![no_main]
#![no_std]

// use defmt_rtt as _;
use panic_halt as _;

use rtic::app;

#[app(device = microbit::pac, dispatchers=[SWI0_EGU0], peripherals = true)]
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
        debouncers: [Debouncer; 2],
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        rtt_init_print!();
        let mut board = Board::new(cx.device, cx.core);

        let clocks = Clocks::new(board.CLOCK);
        clocks.enable_ext_hfosc();

        toggle(&mut board.display_pins.row1);
        toggle(&mut board.display_pins.col3);

        // LED display timer
        let mut timer0 = Timer::periodic(board.TIMER0);
        timer0.start(1_000_000u32);
        timer0.enable_interrupt();

        // button debounce timer
        // check buttons every 5 ms
        // register a Pressed event after stable low state for 10 ms
        // then register a Released event after stable high state for 100 ms
        let mut timer1 = Timer::periodic(board.TIMER1);
        timer1.start(5_000u32);
        timer1.enable_interrupt();
        let debouncers = [
            Debouncer::new(2, 20),
            Debouncer::new(2, 20)
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

    #[task(binds = TIMER0, local = [display_timer/*, pins*/])]
    fn handle_display_timer(cx: handle_display_timer::Context) {
        let _ = cx.local.display_timer.wait(); // consume the event
        //let pins = cx.local.pins;
        //toggle(&mut pins.col3);
        rprintln!("timer 0 ticked !");
    }

    #[task(binds = TIMER1, local = [debounce_timer, buttons, debouncers])]
    fn handle_debounce_timer(cx: handle_debounce_timer::Context) {
        // TODO: better to clear the event here or at end of function?
        let _ = cx.local.debounce_timer.wait(); // consume the event
        let events = [
            ev_for_btn_state(
                BtnIds::BtnA,
                read_debounced_button(&cx.local.buttons.button_a, &mut cx.local.debouncers[0])),
            ev_for_btn_state(
                BtnIds::BtnB,
                read_debounced_button(&cx.local.buttons.button_b, &mut cx.local.debouncers[1])),
        ];

        for v in events.into_iter() {
            if let Some(ev) = v {
                handle_input_event::spawn(ev).unwrap();
            }
        }
    }

    // TODO: bug if both events fire simultaneously
    #[task(priority = 1, local = [pins])]
    fn handle_input_event(cx: handle_input_event::Context, ev: InputEvent) {
        match ev {
            InputEvent::BtnAPressed => cx.local.pins.col1.set_low().void_unwrap(),
            InputEvent::BtnAReleased => cx.local.pins.col1.set_high().void_unwrap(),
            InputEvent::BtnBPressed => cx.local.pins.col5.set_low().void_unwrap(),
            InputEvent::BtnBReleased => cx.local.pins.col5.set_high().void_unwrap(),
        }
        rdbg!(ev);
    }

    fn read_debounced_button(btn: &dyn InputPin<Error = Void>, debouncer: &mut Debouncer) -> Option<BtnState> {
        let raw_state = if btn.is_high().void_unwrap() { BtnState::NotPressed } else { BtnState::Pressed };
        return debouncer.update(raw_state);
    }

    fn ev_for_btn_state(btn_id: BtnIds, btn_state: Option<BtnState>) -> Option<InputEvent> {
        match (btn_id, btn_state) {
            (BtnIds::BtnA, Some(BtnState::Pressed)) => Some(InputEvent::BtnAPressed),
            (BtnIds::BtnB, Some(BtnState::Pressed)) => Some(InputEvent::BtnBPressed),
            (BtnIds::BtnA, Some(BtnState::NotPressed)) => Some(InputEvent::BtnAReleased),
            (BtnIds::BtnB, Some(BtnState::NotPressed)) => Some(InputEvent::BtnBReleased),
            (_, None) => None
        }
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

#[derive(PartialEq, Copy, Clone, Debug)]
enum BtnIds {
    BtnA,
    BtnB,
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum InputEvent {
    BtnAPressed,
    BtnAReleased,
    BtnBPressed,
    BtnBReleased,
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
