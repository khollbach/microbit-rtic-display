#![allow(unused_imports)]
#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

// use defmt_rtt as _;
use panic_halt as _;
//use panic_rtt_target as _;

use rtic::app;

#[app(device = microbit::pac, dispatchers=[SWI0_EGU0], peripherals = true)]
mod app {
    use super::*;

    use core::iter::zip;
    use cortex_m::asm;
    use heapless::Vec;
    use microbit::{
        board::{Board, Buttons, Pins},
        display::nonblocking::{Display, GreyscaleImage},
        gpio,
        hal::{
            clocks::Clocks,
            gpio::{p0, Level, Output, Pin, PushPull},
            gpiote::{Gpiote, GpioteChannel},
            prelude::*,
            rtc::{Rtc, RtcInterrupt, RtcCompareReg},
            timer::Instance,
            timer::Periodic,
            Timer,
        },
        pac, Peripherals,
    };
    use rtic_sync::{channel::*, make_channel};
    use rtt_target::{rdbg, rprintln, rtt_init_print};
    use void::{ResultVoidExt, Void};

    const INPUTQ_CAPACITY: usize = 16;
    type InputQueueSender = Sender<'static, InputEvent, INPUTQ_CAPACITY>;
    type InputQueueReceiver = Receiver<'static, InputEvent, INPUTQ_CAPACITY>;

    //const HEART_IMAGE: [[bool; 5]; 5] = [
    //[false, true , false, true , false],
    //[true , false, true , false, true ],
    //[true , false, false, false, true ],
    //[false, true , false, true , false],
    //[false, false, true , false, false],
    //];

    const HEART_IMAGE: [[u8; 5]; 5] = [
        [0, 15, 0, 15, 0],
        [15, 1, 15, 1, 15],
        [15, 3, 3, 3, 15],
        [0, 15, 1, 15, 0],
        [0, 0, 15, 0, 0],
    ];

    #[shared]
    struct Shared {
        game_state: GameState,
    }

    pub struct GameState {
        run_state: RunState,
        spaceship_x: usize,
        enemies: [bool; 5],
        shots: Vec<(usize, usize), 25>,
    }

    #[derive(PartialEq, Copy, Clone, Debug)]
    enum RunState {
        Running,
        Victory
    }

    impl GameState {
        fn new() -> Self {
            Self {
                run_state: RunState::Running,
                spaceship_x: 2,
                enemies: [true; 5],
                shots: Vec::new(),
            }
        }
    }

    /*
    type DisplayBuffer = [[bool; 5]; 5];

    struct ScreenState {
        buffers: [DisplayBuffer; 2],
        active_buffer: usize,
    }

    struct ScreenReader
    struct ScreenWriter

    impl ScreenState {
        fn new() -> (ScreenReader, ScreenWriter)
    }

    impl ScreenWriter {
        fn swap(&mut self) {
            unsafe {
                active_buffer += 1;
                active_buffer %= 2;
            }
        }

        fn buffer(&mut self) -> &mut DisplayBuffer
    }

    impl ScreenReader {
        fn buffer(&self) -> &DisplayBuffer
    }
    */

    type ScopePin = p0::P0_02<Output<PushPull>>;

    #[local]
    struct Local {
        display_timer: Timer<pac::TIMER0, Periodic>,
        debounce_timer: Timer<pac::TIMER1, Periodic>,
        buttons: Buttons,
        scope_pin: ScopePin,
        debouncers: [Debouncer; 2],
        inputq: InputQueueSender,
        display_rows: [Pin<Output<PushPull>>; 5],
        display_cols: [Pin<Output<PushPull>>; 5],
        rtc: Rtc<pac::RTC0>,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        rtt_init_print!();
        let mut board = Board::new(cx.device, cx.core);

        let clocks = Clocks::new(board.CLOCK);
        clocks.enable_ext_hfosc();

        toggle(&mut board.display_pins.row1);
        toggle(&mut board.display_pins.col3);

        let scope_pin = board.pins.p0_02.into_push_pull_output(Level::Low);

        // LED display timer
        let mut timer0 = Timer::periodic(board.TIMER0);
        timer0.start(4u32); // in microseconds
        timer0.enable_interrupt();

        // button debounce timer
        // check buttons every 5 ms
        // register a Pressed event after stable low state for 10 ms
        // then register a Released event after stable high state for 100 ms
        let mut timer1 = Timer::periodic(board.TIMER1);
        timer1.start(5_000u32);
        timer1.enable_interrupt();
        let debouncers = [Debouncer::new(2, 20), Debouncer::new(2, 20)];

        let (s, r) = make_channel!(InputEvent, INPUTQ_CAPACITY);
        handle_input_event::spawn(r).unwrap();

        // note the order!
        let (display_cols, display_rows) = board.display_pins.degrade();

        let mut rtc = Rtc::new(board.RTC0, 4096 - 1).unwrap();
        rtc.enable_counter();
        rtc.set_compare(RtcCompareReg::Compare0, 4).unwrap();
        rtc.enable_event(RtcInterrupt::Compare0);
        rtc.enable_interrupt(RtcInterrupt::Compare0, None);

        (
            Shared {
                game_state: GameState::new(),
            },
            Local {
                display_timer: timer0,
                debounce_timer: timer1,
                buttons: board.buttons,
                scope_pin: scope_pin,
                debouncers: debouncers,
                inputq: s,
                display_rows,
                display_cols,
                rtc,
            },
        )
    }

    #[task(binds = RTC0, shared = [game_state], local = [rtc])]
    fn game_tick(mut cx: game_tick::Context) {
        cx.local.rtc.reset_event(RtcInterrupt::Compare0);
        cx.local.rtc.clear_counter();

        cx.shared.game_state.lock(|game_state| {
            game_state.shots.retain_mut(|(x, y)| if *y <= 0 {
                // off the screen
                false
            } else {
                *y -= 1;
                if *y == 0 && game_state.enemies[*x] {
                    // hit an alien
                    game_state.enemies[*x] = false;
                    false
                } else {
                    true
                }
            });

            if game_state.enemies.iter().all(|&x| !x) {
                game_state.run_state = RunState::Victory;
            }
        });

        rdbg!("game tick");
    }

    // bit-bash 4-bit pwm
    #[task(binds = TIMER0, shared = [game_state], local = [
        display_timer, display_rows, display_cols, active_row: u8 = 0, display_ticks: u8 = 0, scope_pin])]
    fn handle_display_timer(mut cx: handle_display_timer::Context) {
        cx.local.scope_pin.set_low().void_unwrap();

        let _ = cx.local.display_timer.wait(); // consume the event

        // let display_buf = &HEART_IMAGE;
        let display_buf = cx.shared.game_state.lock(|game_state| {
            let mut buf = [[0; 5]; 5];

            if game_state.run_state == RunState::Victory {
                return [[15; 5]; 5];
            }

            buf[4][game_state.spaceship_x] = 15;
            buf[0] = game_state.enemies.map(|v| if v { 7 } else { 0 });

            for &(x, y) in &game_state.shots {
                buf[y][x] = 3;
            }

            buf
        });

        let active_row = cx.local.active_row;
        let ticks = cx.local.display_ticks;

        *ticks += 1;
        if *ticks > 15 {
            *ticks = 0;
        }

        // clear the previous row
        if *ticks == 0 {
            cx.local.display_rows[*active_row as usize]
                .set_low()
                .void_unwrap();

            *active_row += 1;
            if *active_row >= 5 {
                *active_row = 0;
            }
        }

        // set column values for new row
        for (col, brightness) in zip(cx.local.display_cols, display_buf[*active_row as usize]) {
            if brightness > 0 && brightness >= *ticks {
                col.set_low().void_unwrap(); // led on
            } else {
                col.set_high().void_unwrap(); // led off
            }
        }

        // activate the new row
        if *ticks == 0 {
            cx.local.display_rows[*active_row as usize]
                .set_high()
                .void_unwrap();
        }

        cx.local.scope_pin.set_high().void_unwrap();
    }

    // #[task(binds = TIMER0, local = [
    //     display_timer, display_rows, display_cols, active_row: u8 = 0, scope_pin])]
    // fn handle_display_timer(cx: handle_display_timer::Context) {
    //     cx.local.scope_pin.set_low().void_unwrap();
    //     let _ = cx.local.display_timer.wait(); // consume the event

    //     let display_buf = &HEART_IMAGE;

    //     let active_row = cx.local.active_row;

    //     // clear the previous row
    //     cx.local.display_rows[*active_row as usize].set_low().void_unwrap();

    //     *active_row += 1;
    //     if *active_row >= 5 {
    //         *active_row = 0;
    //     }

    //     // set column values for new row
    //     for (col, brightness) in zip(cx.local.display_cols, display_buf[*active_row as usize]) {
    //         if brightness > 0 {
    //             col.set_low().void_unwrap();    // led on
    //         } else {
    //             col.set_high().void_unwrap();   // led off
    //         }
    //     }

    //     //toggle(&mut cx.local.display_cols[0]);

    //     // activate the new row
    //     cx.local.display_rows[*active_row as usize].set_high().void_unwrap();
    //     cx.local.scope_pin.set_high().void_unwrap();
    // }

    #[task(binds = TIMER1, local = [debounce_timer, buttons, debouncers, inputq])]
    fn handle_debounce_timer(cx: handle_debounce_timer::Context) {
        // TODO: better to clear the event here or at end of function?
        let _ = cx.local.debounce_timer.wait(); // consume the event
        let events = [
            ev_for_btn_state(
                BtnIds::BtnA,
                read_debounced_button(&cx.local.buttons.button_a, &mut cx.local.debouncers[0]),
            ),
            ev_for_btn_state(
                BtnIds::BtnB,
                read_debounced_button(&cx.local.buttons.button_b, &mut cx.local.debouncers[1]),
            ),
        ];

        for v in events.into_iter() {
            if let Some(ev) = v {
                //handle_input_event::spawn(ev).unwrap();
                cx.local.inputq.try_send(ev).ok();
            }
        }
    }

    #[task(priority = 1, shared = [game_state])]
    async fn handle_input_event(
        mut cx: handle_input_event::Context,
        mut inputqr: InputQueueReceiver,
    ) {
        while let Ok(ev) = inputqr.recv().await {
            rdbg!(ev);
            cx.shared.game_state.lock(|state| {
                if state.run_state == RunState::Victory {
                    if ev == InputEvent::BtnAPressed || ev == InputEvent::BtnBPressed {
                        *state = GameState::new();
                    }
                    return;
                }

                match ev {
                    InputEvent::BtnAPressed => {
                        if state.spaceship_x > 0 {
                            state.spaceship_x -= 1;
                        }
                    }
                    InputEvent::BtnAReleased => (),
                    InputEvent::BtnBPressed => {
                        if state.spaceship_x + 1 < 5 {
                            state.spaceship_x += 1;
                        }
                    }
                    InputEvent::BtnBReleased => {
                        let new_shot = (state.spaceship_x, 3);

                        if !state.shots.contains(&new_shot) {
                            state.shots.push(new_shot).unwrap();
                        }
                    }
                }
                // rdbg!(state.spaceship_x);
            });
        }
    }

    fn read_debounced_button(
        btn: &dyn InputPin<Error = Void>,
        debouncer: &mut Debouncer,
    ) -> Option<BtnState> {
        let raw_state = if btn.is_high().void_unwrap() {
            BtnState::NotPressed
        } else {
            BtnState::Pressed
        };
        return debouncer.update(raw_state);
    }

    fn ev_for_btn_state(btn_id: BtnIds, btn_state: Option<BtnState>) -> Option<InputEvent> {
        match (btn_id, btn_state) {
            (BtnIds::BtnA, Some(BtnState::Pressed)) => Some(InputEvent::BtnAPressed),
            (BtnIds::BtnB, Some(BtnState::Pressed)) => Some(InputEvent::BtnBPressed),
            (BtnIds::BtnA, Some(BtnState::NotPressed)) => Some(InputEvent::BtnAReleased),
            (BtnIds::BtnB, Some(BtnState::NotPressed)) => Some(InputEvent::BtnBReleased),
            (_, None) => None,
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
