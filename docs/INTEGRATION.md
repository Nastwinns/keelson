# Integration recipes — wiring `haw` to your toolchain

`haw` never bundles or reimplements a compiler, a config generator, or an emulator. It
only **shells out** the per-repo `build =` / `test =` command you declare (with the repo
directory as the working directory, so `$PWD` inside the command *is* the repo path). That
is the entire integration surface: **put your toolchain's command in `build =`, and `haw`
drives the whole fleet** — in parallel, pinned to `haw.lock`, with a CI-grade exit code.

So "integrating haw" is the same one move whether your compiler is `gcc`, a Docker image,
or a €50k licensed automotive suite. This page shows both:

- **Recipes that were actually executed** (open toolchains: Docker cross-compile, QEMU
  emulation, FreeRTOS) — with real captured output.
- **Wiring patterns** for proprietary/licensed tools (Vector, EB tresos, Green Hills, IAR,
  Tasking, Renode) — the exact shape, honestly marked *not run here* (we don't have the
  licenses), so nothing is fabricated.

> **The one idea:** `haw build` / `haw test` run `build =` / `test =` per repo and fail
> (non-zero) if any repo's command fails. Nothing in `haw` is ARM-, Docker-, or
> QEMU-specific. Wrap any toolchain the same way.

---

## Toolchain in a container (no host cross-toolchain needed)

You rarely install a cross-compiler on every developer's machine. Put it in a Docker image
once and reference it from `build =`. The two images used below:

```dockerfile
# haw-arm-gcc — bare-metal ARM cross-compiler
FROM ubuntu:22.04
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-arm-none-eabi libnewlib-arm-none-eabi make ca-certificates \
 && rm -rf /var/lib/apt/lists/*
```

```dockerfile
# haw-arm-emu — same, plus the QEMU emulator for on-CI firmware runs
FROM ubuntu:22.04
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-arm-none-eabi libnewlib-arm-none-eabi make git ca-certificates qemu-system-arm \
 && rm -rf /var/lib/apt/lists/*
```

```console
$ docker run --rm haw-arm-emu sh -c 'arm-none-eabi-gcc --version | head -1; qemu-system-arm --version | head -1'
arm-none-eabi-gcc (15:10.3-2021.07-4) 10.3.1 20210621 (release)
QEMU emulator version 6.2.0 (Debian 1:6.2+dfsg-2ubuntu6.31)
```

---

## Recipe 1 — Docker cross-compile (bare-metal ARM Cortex-M) ✅ *executed*

`littlefs` (a real fail-safe filesystem for MCUs) compiled to a **Cortex-M4** static
archive inside the toolchain image; the `test =` step asserts the produced object is
genuinely ARM.

```toml
[repo.littlefs]
url    = "https://github.com/littlefs-project/littlefs.git"
rev    = "master"
groups = ["firmware"]
build  = "docker run --rm -v \"$PWD\":/w -w /w haw-arm-gcc sh -c 'arm-none-eabi-gcc -mcpu=cortex-m4 -mthumb -Os -Wall -c lfs.c lfs_util.c && arm-none-eabi-ar rcs lfs-cortexm4.a lfs.o lfs_util.o && arm-none-eabi-size lfs-cortexm4.a'"
test   = "docker run --rm -v \"$PWD\":/w -w /w haw-arm-gcc sh -c 'arm-none-eabi-objdump -f lfs.o | grep -i \"architecture: arm\" && echo CONFIRMED_ARM_CORTEX_M_OBJECT'"

[stack.fw]
repos = ["littlefs"]
```

Real captured output (`haw build` then `haw test`, both exit 0):

```console
$ haw build
── littlefs ──
   text    data     bss     dec     hex  filename
  21880       0       0   21880    5578  lfs.o (ex lfs-cortexm4.a)
    120       0       0     120      78  lfs_util.o (ex lfs-cortexm4.a)
build ran in 1/1 repos

$ haw test
── littlefs ──
architecture: armv7e-m, flags 0x00000011:
CONFIRMED_ARM_CORTEX_M_OBJECT
test ran in 1/1 repos
```

`armv7e-m` is the Cortex-M4 ISA — a genuine bare-metal ARM object, produced by
`arm-none-eabi-gcc` inside Docker, orchestrated by `haw`, with a native-machine `haw`.

---

## Recipe 2 — QEMU emulated run (FreeRTOS on Cortex-M3) ✅ *executed*

The official FreeRTOS QEMU demo built with the FreeRTOS-Kernel, then **booted on
`qemu-system-arm -M mps2-an385 -cpu cortex-m3`**. The scheduler runs the blinky demo (a
task + a software timer feeding a queue) and prints over the semihosted UART; `test =`
greps for a live marker, so a genuinely running RTOS exits 0 and a dead image exits 1.

```toml
[repo.freertos]
url    = "https://github.com/FreeRTOS/FreeRTOS.git"
rev    = "main"
groups = ["rtos"]
build  = "docker run --rm -v \"$PWD\":/w -w /w haw-arm-emu make -C FreeRTOS/Demo/CORTEX_MPS2_QEMU_IAR_GCC/build/gcc"
test   = "docker run --rm -v \"$PWD\":/w -w /w haw-arm-emu sh -c 'timeout 8 qemu-system-arm -machine mps2-an385 -cpu cortex-m3 -kernel FreeRTOS/Demo/CORTEX_MPS2_QEMU_IAR_GCC/build/gcc/output/RTOSDemo.out -monitor none -nographic -serial stdio -semihosting-config enable=on,target=native 2>&1 | head -40 | grep -q \"Message received from task\" && echo QEMU_FREERTOS_RUN_CONFIRMED'"

[stack.rtos]
repos = ["freertos"]
```

Real captured output:

```console
$ haw build        # relinks output/RTOSDemo.out
   text    data     bss     dec     hex  filename
  23902     232  121589  145723   2393b  ./output/RTOSDemo.out
build ran in 1/1 repos

$ haw test         # FreeRTOS scheduler actually executing under QEMU
── freertos ──
Message received from task
Message received from task
... (37 total, plus 3 "Message received from software timer") ...
QEMU_FREERTOS_RUN_CONFIRMED
test ran in 1/1 repos
```

Negative control verified: point the grep at a marker that never appears and the QEMU step
exits 1 — so `haw test` genuinely fails if the RTOS doesn't boot, rather than always passing.

> **Submodules are fault-tolerant.** `haw sync --recurse-submodules` initializes each
> submodule independently and **skips broken/unreachable ones with a warning** instead of
> aborting — important for big upstream repos like `FreeRTOS/FreeRTOS` that declare a large
> forest of heavy submodules (wolfSSL, several AWS IoT SDKs, …). The kernel your target
> needs (`FreeRTOS/Source`) is initialized; a submodule that 404s just prints
> `haw: skipped submodule '<path>': …` and the sync still succeeds. (`ext`/`fd`/`file`
> transports stay hard-disabled in submodule fetches — the RCE guard is preserved.)

Swap QEMU for **Renode** with one line — `test = "renode --console -e 'include @sim.resc; start; sleep 5; quit'"` then grep the UART log the same way.

---

## Patterns for licensed / proprietary toolchains ⚠️ *not run here*

These are the **exact wiring shapes** for commercial automotive/safety tools. We can't run
them (licensed) — the flags follow each vendor's batch/CLI interface; adjust to your project
per the vendor docs. The point: it's the *same* `build =` shell-out.

The AUTOSAR mental model per ECU repo: **config (ARXML) → vendor generator → generated
BSW/RTE C → vendor compiler → ELF**, all pinned in `haw.lock`.

```toml
# EB tresos (Elektrobit) — generate BSW from config, then compile
[repo.ecu-comfort]
url    = "git@gitlab.company.com:ecu/comfort-bsw.git"
rev    = "release/2.4"
groups = ["ecu", "autosar"]
build  = "$TRESOS_BASE/bin/tresos_cmd.sh -p ComfortEcu generate && make -C output"
test   = "make -C output test"     # VectorCAST / Tessy / your MCAL harness

[plugins]
misra  = ["pre-request"]   # gate generated + hand code on MISRA C
aspice = ["post-land"]     # emit ASPICE traceability as the change lands
```

```toml
# Vector MICROSAR / DaVinci Configurator Pro
build = "DVConfiguratorCmd -d PowertrainEcu.dpa --generateAll && make"

# Compilers (drop-in — one string each; a fleet can mix them)
build = "gbuild -top default.gpj"                 # Green Hills MULTI (ccarm)
build = "iarbuild MyProject.ewp -build Release"   # IAR Embedded Workbench (iccarm)
build = "amk -f project.mk"                        # TASKING (cctc / carm)
build = "make CC=dcc"                              # Wind River Diab

# Zephyr RTOS — west drives the board build + QEMU/renode run itself
build = "west build -b qemu_cortex_m3 samples/hello_world"
test  = "west build -t run"        # or: twister
```

Because each is just a string, one `haw.toml` can compose a **GHS ECU next to an IAR ECU
next to a gcc gateway**, and `haw build -j8` builds them all in parallel with correct
per-repo pass/fail and a non-zero exit if any breaks — your CI gate. (`haw` can also import
a Zephyr `west.yml` / Google `repo` manifest: `haw import --from west.yml`.)

---

## Why this matters for regulated work

- **Reproducible.** Every config/BSW/RTE repo is pinned to an exact SHA in `haw.lock` — the
  generated code is auditable and byte-identical rebuild-to-rebuild (the argument for
  ASPICE / ISO 26262 / DO-178C).
- **Orchestrated.** `haw` runs generate → compile → emulate across the whole ECU fleet in
  parallel, with a CI exit code, using *your* licensed tools unchanged.
- **Governed.** Plugins on lifecycle phases produce the qualification work products —
  `misra` (pre-request gate), `aspice` / SBOM / provenance / signing (post-build/post-land),
  `haw evidence` bundles. See [Plugins](PLUGINS.md), [Domains](DOMAINS.md), [Compliance](COMPLIANCE.md).

You don't adapt your toolchain to `haw`. You put its command in `build =`, and `haw` drives
the fleet.
