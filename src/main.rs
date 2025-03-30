#![no_std]
#![no_main]

use core::{cell::RefCell, fmt::Write, sync::atomic::AtomicU32};

use bsp::entry;
use cortex_m::interrupt::Mutex;
use embedded_hal::{
    digital::{OutputPin, StatefulOutputPin as _},
    pwm::SetDutyCycle,
};
use panic_halt as _;

use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
    gpio::bank0::{Gpio14, Gpio15, Gpio28},
    gpio::{FunctionSio, Pin, PullDown, SioOutput},
    pac,
    pac::interrupt,
    pwm::{FreeRunning, Pwm6, Slice},
    sio::Sio,
    watchdog::Watchdog,
};

use rp2040_hal::binary_info;
use usb_device::{
    bus::UsbBusAllocator,
    device::{StringDescriptors, UsbDeviceBuilder, UsbDeviceState, UsbVidPid},
};
use usbd_serial::SerialPort;
use waveshare_rp2040_zero::{
    self as bsp,
    hal::{gpio::OutputDriveStrength, usb::UsbBus},
};

use heapless::String;

type IrqVars = (
    Slice<Pwm6, FreeRunning>,
    Pin<Gpio28, FunctionSio<SioOutput>, PullDown>,
    Pin<Gpio14, FunctionSio<SioOutput>, PullDown>,
    Pin<Gpio15, FunctionSio<SioOutput>, PullDown>,
);

static GLOBAL_PWM: Mutex<RefCell<Option<IrqVars>>> = Mutex::new(RefCell::new(None));

// [ 1, 14, 0, 1 ],
// [ 1, 1, 13, 1 ],
// [ 0, 0, 2, 14 ],
// [ 14, 1, 1, 0 ],

static TRANSITION_MATRIX: [[usize; 16]; 4] = [
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3],
    [0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3],
    [2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2],
];

static TRANSITION_MATRIX_STEADY: [[usize; 16]; 4] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    [3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

static TRANSITION_COUNT: [AtomicU32; 4] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

const LG_SUBSTATES: usize = 1;

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // External high-speed crystal on the pico board is 12Mhz
    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let pins = bsp::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let led_pin = pins.gp28.into_push_pull_output();
    let mut pin14 = pins.gp14.into_push_pull_output();
    let mut pin15 = pins.gp15.into_push_pull_output();

    pin14.set_drive_strength(OutputDriveStrength::TwoMilliAmps);
    pin15.set_drive_strength(OutputDriveStrength::TwoMilliAmps);

    // Configure PWM with a period of 250 milliseconds
    let pwm_slices = bsp::hal::pwm::Slices::new(pac.PWM, &mut pac.RESETS);
    let mut pwm = pwm_slices.pwm6;
    // 2 us per tick @ 125 MHz
    pwm.set_div_int(125);
    pwm.set_ph_correct();
    pwm.output_to(pins.gp29);
    pwm.channel_b.set_duty_cycle_percent(50).unwrap();
    // 125 ms period = 8 Hz
    pwm.set_top(62_500);
    pwm.enable_interrupt();
    pwm.enable();

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    // Set up the USB Communications Class Device driver
    let mut serial = SerialPort::new(&usb_bus);

    // Create a USB device with a fake VID and PID
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x2E8A, 0x0004))
        .strings(&[StringDescriptors::default()
            .manufacturer("Aperture Science Laboratories")
            .product("Autonomous Temporal Indicator")
            .serial_number("VENTINARI")])
        .unwrap()
        .device_class(2)
        .build();

    cortex_m::interrupt::free(|cs| {
        GLOBAL_PWM
            .borrow(cs)
            .replace(Some((pwm, led_pin, pin14, pin15)));
    });

    // Unmask the PWM interrupt
    unsafe {
        pac::NVIC::unmask(pac::Interrupt::PWM_IRQ_WRAP);
    }

    loop {
        // Wait for the interrupt to be triggered
        cortex_m::asm::wfi();

        let mut buf: String<32> = String::new();

        if usb_dev.poll(&mut [&mut serial]) {}

        if usb_dev.state() == UsbDeviceState::Configured {
            let count_0 = TRANSITION_COUNT[0].load(core::sync::atomic::Ordering::SeqCst);
            let count_1 = TRANSITION_COUNT[1].load(core::sync::atomic::Ordering::SeqCst);
            let count_2 = TRANSITION_COUNT[2].load(core::sync::atomic::Ordering::SeqCst);
            let count_3 = TRANSITION_COUNT[3].load(core::sync::atomic::Ordering::SeqCst);
            let total = count_0 + count_1 + count_2 + count_3;
            let ratio = 4.0 * count_0 as f32 / total as f32;

            write!(
                &mut buf,
                "{:0.03} {} {} {} {} {}\r\n",
                ratio, count_0, count_1, count_2, count_3, total
            )
            .ok();
            serial.write(buf.as_bytes()).ok();
        }
    }
}

#[interrupt]
fn PWM_IRQ_WRAP() {
    static mut VALS: Option<IrqVars> = None;
    static mut STATE: usize = 0;
    static mut LFSR_STATE: u32 = 0xACE1;
    static mut TOCK: bool = false;

    // Update the LFSR state
    let mut rand4 = 0usize;
    for _ in 0..4 {
        let lsb = *LFSR_STATE & 1;
        *LFSR_STATE >>= 1;

        if lsb != 0 {
            *LFSR_STATE ^= 0xB400;
        }

        rand4 = (rand4 << 1) | (lsb as usize);
    }

    // This is one-time lazy initialization
    if VALS.is_none() {
        cortex_m::interrupt::free(|cs| {
            *VALS = GLOBAL_PWM.borrow(cs).take();
        });
    }

    if let Some((pwm, led, pin14, pin15)) = VALS {
        pwm.clear_interrupt();

        *STATE += 1;
        if *STATE % (1 << LG_SUBSTATES) == 0 {
            *STATE = TRANSITION_MATRIX[(*STATE >> LG_SUBSTATES) - 1][rand4] << LG_SUBSTATES;

            let index = (*STATE >> LG_SUBSTATES) as usize;
            let current = TRANSITION_COUNT[index].load(core::sync::atomic::Ordering::SeqCst);
            TRANSITION_COUNT[index].store(current + 1, core::sync::atomic::Ordering::SeqCst);
        }

        if *STATE == 0 {
            led.set_high().unwrap();
            if *TOCK {
                pin14.set_high().unwrap();
                pin15.set_low().unwrap();
            } else {
                pin14.set_low().unwrap();
                pin15.set_high().unwrap();
            }
            *TOCK = !*TOCK;
        } else {
            pin14.set_low().unwrap();
            pin15.set_low().unwrap();
            led.set_low().unwrap();
        }
    }
}

#[link_section = ".bi_entries"]
#[used]
pub static PICOTOOL_ENTRIES: [binary_info::EntryAddr; 6] = [
    binary_info::rp_program_name!(c"Ventinari's clock"),
    binary_info::rp_cargo_version!(),
    binary_info::rp_program_description!(c"A stuttering clock"),
    binary_info::rp_program_url!(c"https://github.com/iracigt/ventinari-clock"),
    binary_info::rp_program_build_attribute!(),
    binary_info::rp_pico_board!(c"pico"),
];
