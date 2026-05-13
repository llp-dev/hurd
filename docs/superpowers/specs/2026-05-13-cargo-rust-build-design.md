# Isolated Cargo Workspace for Hurd Rust Components

**Status:** Design approved, awaiting implementation plan
**Date:** 2026-05-13
**Branch:** `rust`

## Problem

The current `rust` branch builds `init/init.rs` by invoking `rustc` directly from the existing autoconf + Make build system. Three files were modified to enable this: `configure.ac` (added `AC_CHECK_TOOL(RUSTC, rustc)` and a `rust_target` mapping), `config.make.in` (exposed `RUSTC` and `rust_target` as Make variables), and `init/Makefile` (added a `RUSTFLAGS` block and an `init.o: init.rs` rule using `--crate-type=staticlib --emit=obj`). The resulting `.o` is linked by Hurd's standard Makeconf recipe, identical to the C version.

This works, but it does not scale to bigger ports. The next Rust component (procfs is the candidate) will need: multiple Rust source files, shared FFI declarations, a heap allocator, the standard library, and a way to express dependencies between Rust crates. None of those fit naturally into the rustc-direct + Make model.

## Goal

Introduce cargo as the build system for *all* Rust code in Hurd, structured so that adding a new Rust component (procfs and beyond) requires no changes to the build infrastructure itself — only adding a directory under `crates/`. The cargo workspace is **fully isolated** from Hurd's existing autoconf + Make system. The two build systems coexist in the same source tree but share no configuration and no rules.

## Non-goals

- Replacing Hurd's autoconf + Make system for C components. C servers continue to build exactly as they do today.
- Using any external (crates.io) dependencies. The workspace is self-contained; FFI declarations and Hurd glue live in workspace-internal crates.
- Solving generic MIG-to-Rust binding generation. That is a separate future problem; this work does not depend on it and does not preclude it.
- Optimizing init's binary size. The migration is allowed to make the binary larger (libstd is acceptable) because clarity and maintainability matter more for what is fundamentally a small program.

## Architecture

Two disjoint build systems in one source tree:

```
hurd/
├── Cargo.toml                  # workspace declaration
├── Cargo.lock                  # committed
├── .cargo/
│   └── config.toml             # linker config, hand-maintained
├── rust-toolchain.toml         # optional: pins rustc channel
├── .gitignore                  # adds /target/
├── crates/
│   ├── libc/                   # FFI declarations (POSIX/glibc surface)
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── init/                   # the Rust init binary
│       ├── Cargo.toml
│       └── src/main.rs         # was init/init.rs
├── target/                     # cargo output, gitignored
│
├── auth/, ext2fs/, proc/, ...  # untouched C trees
├── Makeconf, configure.ac      # reverted: no Rust references
└── ...
```

Building C parts: `./configure && make`. Building Rust parts: `cargo build --release`. Neither command requires nor uses the other. The only intersection is `debian/rules`, which invokes both during package builds and copies cargo's output binaries into `debian/tmp/hurd/` alongside Make's output.

### Why isolation

- Rust contributors do not need to understand autoconf or Hurd's Makeconf to work on Rust code.
- Cargo's natural workflows (`cargo test`, `cargo clippy`, `cargo fmt`, `rust-analyzer`) work without any wrappers or shims.
- The cargo workspace is reproducible from itself alone — given `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`, and `rustc`, anyone can build the binaries.
- No drift risk: there is no second copy of build configuration that can diverge from a primary source.
- Debian packaging cleanly separates concerns: `dh_auto_configure` and `dh_auto_build` continue to handle C; an `override_dh_auto_build` step invokes cargo.

## Components

### Workspace root — `Cargo.toml`

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.9.0"
edition = "2021"
license = "GPL-2.0-or-later"
repository = "https://git.savannah.gnu.org/git/hurd.git"

[profile.release]
panic = "abort"
codegen-units = 1
opt-level = 3
debug = true
strip = "none"
```

`resolver = "2"` is load-bearing: it prevents accidental feature-unification between `no_std` and `std` consumers as the workspace grows. The release profile sets `panic = "abort"` so that no panic-unwinding tables are linked in; this matches the current rustc-direct build.

### Linker config — `.cargo/config.toml`

```toml
[target.x86_64-unknown-hurd-gnu]
linker = "cc"
rustflags = [
    "-C", "link-arg=-lpthread",
    "-C", "link-arg=-lhurdbugaddr",
]

[target.i686-unknown-hurd-gnu]
linker = "cc"
rustflags = [
    "-C", "link-arg=-lpthread",
    "-C", "link-arg=-lhurdbugaddr",
]
```

Using `cc` as the linker delegates the C-runtime question (crt1, crti, crtn, glibc startup) to gcc, which already knows how to produce a hosted executable on Hurd-gnu. Cargo just runs `cc -o init <objects> <rustflags>` and everything downstream is gcc's responsibility — the same mechanism Rust's own toolchain uses on Tier 3 targets.

We deliberately do **not** set `[build] target = ...`. On the native Hurd VM, cargo's host-triple auto-detection produces the right answer. On Linux cross-compile boxes, contributors pass `--target=x86_64-unknown-hurd-gnu` explicitly (matching Debian's `DEB_HOST_GNU_TYPE`).

### Optional — `rust-toolchain.toml`

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

Pinning the channel signals intent ("stable Rust only — no nightly required") and makes contributor setup uniform.

### libc crate — `crates/libc/`

Holds all POSIX/glibc FFI declarations used by any crate in the workspace. Initial surface (everything `init.rs` currently inlines):

| Category | Items |
|---|---|
| Types | `pid_t`, `error_t`, `sigset_t`, `sigaction_t`, `argp_option`, `argp_t` |
| Process | `getpid`, `fork`, `execv`, `waitpid` |
| I/O | `select` |
| Signals | `sigaction`, `sigemptyset` |
| GNU argp | `argp_parse` |
| Errors / utility | `error`, `strsignal`, `asprintf`, `strdup`, `free`, `__errno_location`, `abort` |

```toml
# crates/libc/Cargo.toml
[package]
name = "libc"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "libc"
# crate-type defaults to "rlib" — what we want for in-workspace consumption
```

The crate is `no_std`-compatible (declarations only, no code) so future no_std consumers can still depend on it.

**Naming caveat:** this name shadows the crates.io `libc` crate. Since the workspace commits to zero external dependencies, the collision cannot manifest at runtime, but tooling that resolves crate documentation may behave oddly. Acceptable trade-off for now; renaming to `hurd-libc` is reversible later.

### init crate — `crates/init/`

```toml
# crates/init/Cargo.toml
[package]
name = "init"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "init"
path = "src/main.rs"

[dependencies]
libc = { path = "../libc" }
```

`src/main.rs` is the current `init/init.rs` adapted as follows:

1. **Remove `#![no_std]`** — use the Rust standard library.
2. **Remove `#![no_main]`** — provide a standard `fn main()`.
3. **Replace inline FFI declarations** with `use libc::{pid_t, sigset_t, sigaction_t, ...}`.
4. **argp parsing** still happens via the existing extern call to glibc's `argp_parse` — we keep the existing argv-handling logic, just routed through `fn main()` that converts `std::env::args_os()` into a `*mut *mut c_char` to pass through.

Behavior must remain identical: same options, same signal mask, same `fork`+`execv` of `${libexecdir}/runsystem.hurd`, same `select`-forever wait loop, same `argv[0]` rewrite to display the runlevel.

The migration accepts that the binary will grow (libstd pulls in additional glibc surface and libpthread becomes a mandatory dynamic dependency). This is acceptable: PID 1's startup cost growing by tens of milliseconds and binary size by a few hundred KB is not a meaningful regression on any system that can run Hurd.

### Future crates (not implemented in this work)

Listed only so the workspace structure's intent is clear:

- `crates/hurd-sys/` — Hurd-specific FFI: libports, libnetfs, libfshelp client surfaces
- `crates/hurd-mig/` — MIG-generated or hand-translated RPC client stubs
- `crates/libnetfs-rs/` — safe Rust wrappers over libnetfs callbacks
- `crates/procfs/` — the next major port

None of these get scaffolded in this work. When they arrive, they slot in as new `members` of the workspace automatically (the `crates/*` glob picks them up).

## Build pipeline

### Developer flow

```sh
cd hurd
cargo build --release
# target/release/init now exists; ELF 64-bit, dynamically linked, for GNU/Hurd
```

`cargo build` runs rustc for `libc` (producing `libc.rlib`), then rustc for `init` (linking against `libc.rlib`), then `cc` as the final linker. Output binary is dynamically linked against `libc.so`, `libpthread.so`, `libhurdbugaddr.so`, and `libgcc_s.so` (the last via cc's standard linkage).

### Install flow

```sh
sudo cp target/release/init /hurd/init
```

Manual `cp` is the v1 install story. If the binary list grows, the workspace gains a tiny `install.sh` script at the root listing the crates → install-path mapping.

### Debian packaging

`debian/rules` adds:

```make
override_dh_auto_build-arch:
    # ... existing dh_auto_build invocations for C parts ...
    cargo build --release --offline

override_dh_auto_install-arch:
    # ... existing dh_auto_install invocations for C parts ...
    install -m 0755 target/release/init debian/tmp/hurd/init
```

`--offline` ensures the build does not attempt network access. Because the workspace has no external dependencies, no vendoring step is required.

## What gets reverted

The migration removes the recent autoconf/Make Rust additions, since they become dead code under the isolated model.

| File | Current state | After migration |
|---|---|---|
| `configure.ac` | Lines 113-121: `AC_CHECK_TOOL(RUSTC, rustc)` + `rust_target` mapping | Reverted to upstream — no Rust references |
| `config.make.in` | Lines 54-55: `RUSTC = @RUSTC@` + `rust_target = @rust_target@` | Reverted — no Rust references |
| `init/Makefile` | RUSTFLAGS block + `init.o: init.rs` rule | Deleted (whole `init/` directory removed) |
| `init/init.rs` | Lives at `init/init.rs` | Moved to `crates/init/src/main.rs` and adapted |
| `init/init.c` | Already deleted in earlier commit | (already done) |

## Failure modes

| Failure | Symptom | Mitigation |
|---|---|---|
| `cc` not found by cargo | "linker `cc` not found" | Install `gcc` package |
| Missing `libpthread` / `libhurdbugaddr` | Undefined references at link | Add `-l...` to rustflags in `.cargo/config.toml` (already specified) |
| Cross-compile from Linux fails | "can't find crate `std` for target …" | `apt install rust-std-x86_64-unknown-hurd-gnu` (Debian); `rustup target add x86_64-unknown-hurd-gnu` |
| `target/` checked into git | bloated repo | `.gitignore` entry `/target/` added in step 1 |
| libstd pulls in unexpected glibc symbols | runtime missing-symbol error on Hurd | Discovered at step 4 (symbol audit); fall back to no_std + nightly `#[start]` if Hurd glibc lacks something libstd needs (low probability — Tier 3 target means libstd is known to work) |

## Verification

The current rustc-direct `init.o`-linked binary is the reference. The cargo-built binary must be functionally equivalent.

1. **Build verification.** `cargo build --release` succeeds and produces `target/release/init` as an ELF for `x86_64-unknown-hurd-gnu`.

2. **Symbol audit.** `objdump -p target/release/init | grep NEEDED` lists the expected shared libraries: `libc.so.0.3`, `libpthread.so.0.3`, `libhurdbugaddr.so.0.3`, plus libstd's runtime libraries (which differ from the reference binary — that is expected and acceptable).

3. **Imported function audit.** `nm -D target/release/init` includes all the FFI calls init makes: `fork`, `execv`, `waitpid`, `select`, `sigaction`, `sigemptyset`, `argp_parse`, `error`, `strsignal`, `asprintf`, `strdup`, `free`, `__errno_location`, `abort`, `getpid`.

4. **Boot test.** Copy the binary to `/hurd/init`, reboot the VM, verify:
   - PID 1 is the new init with the correct `argv[0]` form (`init [2]` style).
   - `ps -ef | head -5` shows the expected boot chain (init → startup → gnumach + proc).
   - The zombie-reaping double-fork test passes: `for i in 1 2 3 4 5; do ( ( sleep 1 ; exit 0 ) & ); done ; sleep 3 ; ps -ef | grep '<defunct>'` produces no output.

5. **Equivalence smoke test.** Same argp options accepted (`--help`, runlevel argument). Same `runsystem.hurd` exec path under `${libexecdir}`. Same select-forever main loop.

## Migration order

A single implementation plan executes these steps; ordering is what reduces risk.

1. Create top-level `Cargo.toml` (workspace declaration), `.cargo/config.toml`, `rust-toolchain.toml`, `.gitignore` update.
2. Create `crates/libc/` with `Cargo.toml` and `src/lib.rs` containing all FFI types and externs currently inlined in `init/init.rs`.
3. Create `crates/init/` with `Cargo.toml` and `src/main.rs` adapted from `init/init.rs`: remove `#![no_std]` and `#![no_main]`, change `extern "C" fn main` to standard Rust `fn main()` that converts argv via `std::env::args_os()`, replace inline FFI with `use libc::*`.
4. Run `cargo build --release` from a clean checkout. Verify the binary builds. Run symbol audit.
5. Boot test on the Hurd VM: backup the working `/hurd/init`, copy the new binary in, reboot, verify init works (including zombie-reaping check).
6. Revert the Rust additions in `configure.ac` and `config.make.in`.
7. Delete the entire `init/` directory.
8. Commit the migration with a clear message and reference to this design doc.

Step 4 is the riskiest atomic step — if `libstd` on Hurd has any gap that prevents init from running (unlikely given Tier 3 target status, but possible), it surfaces there. Recovery: keep the backup of the working `/hurd/init` until step 8.

## Open questions

None at design time. The following are deliberate non-decisions that get resolved during implementation:

- Exact `rustflags` list for `libhurdbugaddr` and any other Hurd-specific link args — start with the listed two and add empirically as link errors surface.
- Whether to commit `Cargo.lock` — yes, since this is application code (a binary), not a library. Committing the lockfile makes the build reproducible.
- Whether `install.sh` exists in v1 — no; manual `cp` until there are 3+ binaries.
