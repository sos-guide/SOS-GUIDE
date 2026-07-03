//! Nœud satellite LoRa SOS-GUIDE — firmware ESP32-C3 (`no_std`).
//!
//! Rôle (cf. CLAUDE.md § `firmware/esp32-lora`) : nœud LoRa basse consommation
//! qui émet/relaie les **alertes** au format de trame v2.5 (JSON compact),
//! interopérable avec le nœud Rust (`sos-core`) et la v2.5 Python.
//!
//! Cet état est un **squelette vérifié à la compilation** (cross-compile RISC-V) :
//! initialisation de la puce, clignotement d'une LED de vie, et construction
//! d'une trame d'alerte sans allocateur. Le pilote radio SX127x (SPI) et la
//! boucle mesh seront ajoutés avec le matériel.

#![no_std]
#![no_main]

use core::fmt::Write;

use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;
use heapless::String;

/// Version du protocole de trame (identique à `sos_core::PROTOCOL_VERSION`).
const PROTOCOL_VERSION: u8 = 1;
/// Taille max d'une trame LoRa (SF7 ≈ 255 octets ; on borne à 250 utiles).
const FRAME_CAP: usize = 250;

/// Construit la trame d'alerte JSON compacte `{"v":1,"nid":…,"typ":…,"ts":…,
/// "hop":0,"msg":…}` — **octet pour octet** comme `sos_core::AlertPacket::to_frame`
/// (champs dans le même ordre), sans allocateur de tas.
fn build_frame(node_id: &str, alert_type: &str, ts: i64, message: &str) -> String<FRAME_CAP> {
    let mut frame: String<FRAME_CAP> = String::new();
    // En cas de débordement, `write!` renvoie une erreur : on émet une trame
    // tronquée plutôt que de paniquer (un nœud doit rester vivant).
    let _ = write!(
        frame,
        "{{\"v\":{PROTOCOL_VERSION},\"nid\":\"{node_id}\",\"typ\":\"{alert_type}\",\"ts\":{ts},\"hop\":0,\"msg\":\"{message}\"}}"
    );
    frame
}

#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    // LED de vie (GPIO8 sur la plupart des cartes ESP32-C3 DevKit).
    let mut led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
    let delay = Delay::new();

    // Trame d'exemple (sera produite par les capteurs/relais réels).
    let _frame = build_frame("esp32-sat-01", "PPMS", 1_748_123_456, "Test LoRa");

    loop {
        led.toggle();
        delay.delay_millis(500);
    }
}

/// Sans `std`, un gestionnaire de panique est obligatoire. On reste bloqué (le
/// watchdog matériel relancera la puce) — pas de dépendance `esp-backtrace`.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
