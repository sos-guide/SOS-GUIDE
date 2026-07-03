# `esp32-lora` — nœud satellite LoRa (firmware ESP32-C3, `no_std`)

Firmware du nœud satellite LoRa de SOS-GUIDE. **Workspace séparé** du nœud Pi
(toolchain et cible différentes ; exclu du workspace hôte et de `just sync`).

## Cible

**ESP32-C3 (RISC-V, `riscv32imc-unknown-none-elf`)** — toolchain Rust **standard**
(pas besoin du fork Xtensa `espup`). Pour un ESP32 classique (Xtensa) ou un C6,
changer la feature `esp32c3` dans `Cargo.toml` et la cible dans `.cargo/config.toml`.

## État

Squelette **vérifié à la compilation** (cross-compile, sans matériel) :
- initialisation de la puce (`esp-hal` 1.x) + LED de vie ;
- `build_frame()` : construction de la trame d'alerte JSON compacte **au format
  v2.5**, sans allocateur (`heapless`), identique octet pour octet à
  `sos_core::AlertPacket::to_frame` — interopérable avec le nœud Pi et la v2.5.

À venir (avec le matériel) : pilote radio SX127x (SPI), boucle mesh (réception,
anti-rejeu, relais), basse consommation (Embassy + deep-sleep).

## Compiler (aucun matériel requis)

```sh
rustup target add riscv32imc-unknown-none-elf
cd firmware/esp32-lora
cargo build --release
```

## Flasher (matériel requis)

```sh
cargo install espflash
cargo run --release   # = espflash flash --monitor (cf. .cargo/config.toml)
```
