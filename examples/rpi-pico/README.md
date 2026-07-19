# rpi-pico — a real Raspberry Pi Pico firmware fleet

Two genuine Raspberry Pi Pico (RP2040) firmware stacks plus an embedded C JSON
library, composed as one `haw` fleet. Every `build =` / `test =` in
[`haw.toml`](haw.toml) was **actually executed with `haw` and seen to succeed
end-to-end on macOS** — the embassy blinky firmware compiled to an ARM
Cortex-M0+ ELF, and cJSON's ctest passed **19/19**. This is a runnable manifest,
not a reading one. `haw sync` clones the real upstreams over HTTPS with no
credentials, and all three have **active GitHub Actions CI** on their default
branch.

The point: the two Pico firmwares cross-compile with **Rust's built-in
`thumbv6m-none-eabi` target — no external ARM GCC toolchain**. If you have
`rustup`, you already have everything.

## The fleet

| Repo | Domain | What builds | Test |
| --- | --- | --- | --- |
| [embassy](https://github.com/embassy-rs/embassy) | rpi-pico | `examples/rp` blinky firmware cross-compiled to **RP2040 / Cortex-M0+** (`thumbv6m-none-eabi`) — a real Pico ELF | build-only |
| [rp-hal](https://github.com/rp-rs/rp-hal) | rpi-pico | the community RP2040 HAL library crate, built for `thumbv6m-none-eabi` | build-only |
| [cJSON](https://github.com/DaveGamble/cJSON) | data | CMake build of the ubiquitous embedded JSON parser | `ctest` — **19/19 pass** |

## One-time setup

The two Pico repos need Rust's ARM target — no external toolchain:

```console
$ rustup target add thumbv6m-none-eabi
```

cJSON just needs `cmake` + a C compiler (Apple clang / gcc), which you likely
already have.

## Run it

```console
$ mkdir /tmp/pico && cp haw.toml /tmp/pico/ && cd /tmp/pico
$ haw sync            # clones all three upstreams (needs network)
$ haw build -j3       # cross-compiles both Pico firmwares + builds cJSON
$ haw test            # runs cJSON's ctest suite (19/19)
```

Sync just the firmware slice with the `pico` stack:

```console
$ haw sync --stack pico    # clones embassy + rp-hal only
$ haw build --stack pico   # two ARM cross-compiles, in parallel
```

## Watch the CI in the cockpit

All three repos have live GitHub Actions. Open the cockpit and jump to the
network views:

```console
$ haw dash
```

- press **`4`** for the fleet-wide **CI runs** view — recent Actions runs across
  all three repos, with live progress; `Enter` drills into a run's jobs and
  steps, `l` reads its logs.
- press **`3`** for the fleet-wide **PR/MR** view.

## Why it's a good example

- **Real cross-compilation, zero toolchain hunting** — `thumbv6m-none-eabi`
  ships with Rust. One `rustup target add` and you're cross-building real Pico
  firmware.
- **Heterogeneous fleet, one exit code** — two Cargo cross-builds and a
  CMake/ctest build driven by a single `haw build -j3` / `haw test`, with one
  CI-grade exit code for the whole fleet.
- **All upstreams alive** — active CI you can watch from `haw dash` → `4`.
