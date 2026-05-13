# Cargo Rust Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate the existing `init/init.rs` (built via the Hurd Make + autoconf system with rustc-direct) into a self-contained cargo workspace under `crates/`, with a shared `libc` FFI crate. The workspace is intentionally isolated from autoconf and Make.

**Architecture:** Top-level `Cargo.toml` workspace, `.cargo/config.toml` (committed) for linker configuration, and `rust-toolchain.toml` (optional channel pin). Two member crates: `crates/libc` (POSIX/glibc FFI declarations, no code), `crates/init` (`[[bin]]` target, depends on libc). The init binary drops both `#![no_std]` and `#![no_main]` and uses libstd with a standard Rust `fn main()`. Cargo invokes `cc` as the linker, which transparently provides crt1, crti, glibc startup, and a NEEDED entry for `libc.so.0.3`.

**Tech Stack:** Rust ≥1.94 stable, cargo ≥1.94, gcc (as `cc`), GNU/Hurd glibc, GNU argp.

**Reference spec:** `docs/superpowers/specs/2026-05-13-cargo-rust-build-design.md`

---

### Task 1: Workspace skeleton

Create the cargo workspace bootstrap files. After this task, `cargo build` runs cleanly but produces no binaries (libc is a placeholder rlib, init does not exist yet).

**Files:**
- Create: `Cargo.toml`
- Create: `.cargo/config.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/libc/Cargo.toml`
- Create: `crates/libc/src/lib.rs`
- Modify: `.gitignore`

- [ ] **Step 1: Create the top-level `Cargo.toml`**

Write file `Cargo.toml` at the repo root:

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

[profile.dev]
panic = "abort"
```

`panic = "abort"` is set in both profiles so the choice is consistent regardless of `--release`. `resolver = "2"` is load-bearing: it prevents accidental feature-unification between `no_std`-capable and `std`-using consumers as the workspace grows.

- [ ] **Step 2: Create the linker config**

Write file `.cargo/config.toml`:

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

`linker = "cc"` delegates the C-runtime startup question (crt1, crti, crtn, glibc bootstrap) to gcc. We deliberately do **not** set `[build] target = ...` — on the Hurd VM cargo auto-detects the host triple; on Linux cross-compile boxes contributors pass `--target=...` explicitly.

- [ ] **Step 3: Create the toolchain pin**

Write file `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

This file is honored by `rustup`-managed installs and silently ignored when rustc is installed directly via apt (as on the Debian/Hurd VM).

- [ ] **Step 4: Create the libc crate manifest**

Write file `crates/libc/Cargo.toml`:

```toml
[package]
name = "libc"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "libc"
# crate-type defaults to ["lib"] which produces an rlib — what we want.
```

- [ ] **Step 5: Create the libc crate stub source**

Write file `crates/libc/src/lib.rs`:

```rust
//! POSIX / GNU-libc FFI declarations used across the Hurd Rust workspace.
//!
//! No code lives here, only `extern "C"` declarations, `#[repr(C)]` types,
//! constants, and small `#[inline]` macros translated from libc headers.
//!
//! This crate has no dependencies and is `no_std`-compatible so that future
//! `no_std` consumers (kernel-adjacent code) can use the same declarations.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]
```

The body will be populated in Task 2. Leaving it empty for now lets us verify the workspace compiles before bringing in real FFI declarations.

- [ ] **Step 6: Update `.gitignore`**

Edit `.gitignore` — add a new line `/target/` after the existing `*.so.*` line (around line 7). The relevant section after the edit:

```
*~

*.d
*.o
*.a
*.so
*.so.*
/target/
TAGS
```

`Cargo.lock` is NOT ignored — for application code (our init binary) we commit it so builds are reproducible.

- [ ] **Step 7: Build to verify the workspace is well-formed**

Run from the repo root:

```sh
cargo build
```

Expected: `Compiling libc v0.9.0 (...crates/libc)` followed by `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in N.NNs`. A `target/debug/` directory now exists with `libc.rlib` inside.

If you see "current package believes it's in a workspace when it's not", you likely have a stray `Cargo.toml` under `crates/` other than `libc/`. Delete it.

- [ ] **Step 8: Commit**

```sh
git add Cargo.toml .cargo/config.toml rust-toolchain.toml .gitignore crates/libc/
git commit -m "build: introduce isolated cargo workspace skeleton

Adds the top-level Cargo.toml + .cargo/config.toml + rust-toolchain.toml
that scaffold a self-contained cargo workspace for Rust components.
crates/libc/ is created as a placeholder for shared FFI declarations and
will be populated in the following commit.

Per the design at docs/superpowers/specs/2026-05-13-cargo-rust-build-design.md,
the cargo workspace and the existing Hurd autoconf+Make build are
intentionally disjoint: neither knows about the other."
```

---

### Task 2: Populate the libc FFI crate

Move all FFI types, externs, and constants currently inlined in `init/init.rs` into `crates/libc/src/lib.rs`. After this task, `cargo build` still succeeds with just the rlib; no binaries are produced yet.

**Files:**
- Modify: `crates/libc/src/lib.rs`

- [ ] **Step 1: Replace `crates/libc/src/lib.rs` with the full FFI surface**

Overwrite `crates/libc/src/lib.rs` with this content:

```rust
//! POSIX / GNU-libc FFI declarations used across the Hurd Rust workspace.
//!
//! No code lives here, only `extern "C"` declarations, `#[repr(C)]` types,
//! constants, and small `#[inline]` macros translated from libc headers.
//!
//! This crate has no dependencies and is `no_std`-compatible so that future
//! `no_std` consumers can use the same declarations.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

pub use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

// ---- types ----

pub type pid_t   = c_int;
pub type error_t = c_int;

// Hurd glibc declares sigset_t as `unsigned long int`.
// See sysdeps/mach/hurd/bits/sigset.h.
pub type sigset_t = c_ulong;

#[repr(C)]
pub struct sigaction_t {
    // Holds either a function pointer or SIG_IGN/SIG_DFL. Glibc treats this
    // slot as a union of (void (*)(int)) and (void (*)(int, siginfo_t*, void*));
    // ABI-wise it's just one pointer-sized word.
    pub sa_handler: usize,
    pub sa_mask:    sigset_t,
    pub sa_flags:   c_int,
    // No sa_restorer on the Hurd — signal trampolining is done in userspace.
}

#[repr(C)]
pub struct argp_option {
    pub name:  *const c_char,
    pub key:   c_int,
    pub arg:   *const c_char,
    pub flags: c_int,
    pub doc:   *const c_char,
    pub group: c_int,
}

pub type argp_parser_t =
    Option<unsafe extern "C" fn(c_int, *mut c_char, *mut c_void) -> error_t>;

#[repr(C)]
pub struct argp_t {
    pub options:     *const argp_option,
    pub parser:      argp_parser_t,
    pub args_doc:    *const c_char,
    pub doc:         *const c_char,
    pub children:    *const c_void,
    pub help_filter: *const c_void,
    pub argp_domain: *const c_char,
}

// ---- functions ----

extern "C" {
    pub fn argp_parse(argp: *const argp_t, argc: c_int, argv: *mut *mut c_char,
                      flags: c_uint, arg_index: *mut c_int,
                      input: *mut c_void) -> error_t;

    pub fn getpid() -> pid_t;
    pub fn fork()   -> pid_t;
    pub fn execv(path: *const c_char, argv: *const *const c_char) -> c_int;
    pub fn waitpid(pid: pid_t, wstatus: *mut c_int, options: c_int) -> pid_t;
    pub fn select(nfds: c_int,
                  readfds:   *mut c_void,
                  writefds:  *mut c_void,
                  exceptfds: *mut c_void,
                  timeout:   *mut c_void) -> c_int;

    pub fn sigaction(signum: c_int, act: *const sigaction_t,
                     oldact: *mut sigaction_t) -> c_int;
    pub fn sigemptyset(set: *mut sigset_t) -> c_int;

    pub fn error(status: c_int, errnum: c_int, format: *const c_char, ...);
    pub fn strsignal(sig: c_int) -> *const c_char;
    pub fn asprintf(strp: *mut *mut c_char, fmt: *const c_char, ...) -> c_int;
    pub fn strdup(s: *const c_char) -> *mut c_char;
    pub fn free(p: *mut c_void);

    // Glibc's errno is thread-local; access it through __errno_location().
    pub fn __errno_location() -> *mut c_int;

    pub fn abort() -> !;
}

#[inline] pub fn errno() -> c_int { unsafe { *__errno_location() } }

// ---- Hurd signal constants (BSD numbering) ----
//
// See sysdeps/mach/hurd/bits/signum-arch.h in glibc. These differ from Linux.

pub const SIGHUP:  c_int =  1;
pub const SIGINT:  c_int =  2;
pub const SIGQUIT: c_int =  3;
pub const SIGTERM: c_int = 15;
pub const SIGTSTP: c_int = 18;
pub const SIGCHLD: c_int = 20;
pub const SIGUSR1: c_int = 30;
pub const SIGUSR2: c_int = 31;

pub const SIG_IGN:    usize = 1;
pub const SA_RESTART: c_int = 0x0002;

pub const WAIT_ANY:  pid_t = -1;
pub const WNOHANG:   c_int = 1;
pub const WUNTRACED: c_int = 2;

// POSIX wait-status decoding. Hurd glibc uses the same encoding.
#[inline] pub fn WTERMSIG(s: c_int)    -> c_int { s & 0x7f }
#[inline] pub fn WIFSIGNALED(s: c_int) -> bool  { (((s & 0x7f) + 1) >> 1) > 0 }
#[inline] pub fn WIFSTOPPED(s: c_int)  -> bool  { (s & 0xff) == 0x7f }
#[inline] pub fn WEXITSTATUS(s: c_int) -> c_int { (s >> 8) & 0xff }

// ARGP_ERR_UNKNOWN == E2BIG. On Hurd, E2BIG = _HURD_ERRNO(7) = 0x40000007
// because errno values are tagged with a sub-system code so the same int
// can carry POSIX and Mach error codes.
pub const ARGP_ERR_UNKNOWN: error_t = 0x40000007;
```

This is identical to the FFI block currently inlined in `init/init.rs`, with two adjustments: items are made `pub`, and `core::ffi::*` is re-exported so dependents can write `use libc::{c_int, c_char}` without also adding `core::ffi` imports.

- [ ] **Step 2: Build to verify the FFI compiles**

```sh
cargo build
```

Expected: `Compiling libc v0.9.0 (...crates/libc)` then `Finished`. No warnings (the `#![allow(...)]` line suppresses naming-convention warnings).

If you see "expected one of `;`, `=`, …" or similar parse errors, there's a typo — re-read the file carefully. The most likely culprits are stray semicolons after `extern "C" { ... }` blocks or missing commas in struct fields.

- [ ] **Step 3: Commit**

```sh
git add crates/libc/src/lib.rs
git commit -m "build(libc): populate FFI declarations for init

Migrates the libc FFI block currently inlined in init/init.rs into the
shared crates/libc crate. Items are made pub so init (and future
consumers) can import via 'use libc::...'. No behavioral change."
```

---

### Task 3: Init crate skeleton

Create `crates/init/` with `Cargo.toml` and a stub `src/main.rs` that produces an actual binary. The binary does nothing useful yet — it just exits 0 — but proves the `[[bin]]` + libc-dependency wiring works.

**Files:**
- Create: `crates/init/Cargo.toml`
- Create: `crates/init/src/main.rs`

- [ ] **Step 1: Create the init crate manifest**

Write file `crates/init/Cargo.toml`:

```toml
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

- [ ] **Step 2: Create the stub main.rs**

Write file `crates/init/src/main.rs`:

```rust
// Stub. Will be replaced in Task 4 with the full port of init/init.rs.

fn main() {
    // Intentionally empty. Exists only so Task 3 can verify the [[bin]]
    // target and libc dependency are wired up correctly.
    let _ = libc::SIGCHLD; // ensure the dependency is actually used
}
```

The `let _ = libc::SIGCHLD` forces cargo to actually link `libc.rlib`, which exercises the cross-crate wiring. Without it, the dependency might be silently elided.

- [ ] **Step 3: Build the binary**

```sh
cargo build --release
```

Expected output:

```
   Compiling libc v0.9.0 (...crates/libc)
   Compiling init v0.9.0 (...crates/init)
    Finished `release` profile [optimized + debuginfo] target(s) in N.NNs
```

Verify the binary exists and is for Hurd:

```sh
file target/release/init
```

Expected: `target/release/init: ELF 64-bit LSB executable, x86-64, ..., dynamically linked, interpreter /lib/ld.so, for GNU/Hurd ...`. If you see "for GNU/Linux" or similar, cargo built for the wrong target — verify you are running on the Hurd VM and that `rustc -vV` shows `host: x86_64-unknown-hurd-gnu`.

If you see linker errors mentioning `pthread_*` or `hurdbugaddr_*`, the `.cargo/config.toml` rustflags from Task 1 are missing or didn't take effect — verify the file content.

- [ ] **Step 4: Commit**

```sh
git add crates/init/
git commit -m "build(init): add init crate skeleton

Adds crates/init/ with a [[bin]] target and a stub main.rs that exits 0.
Verifies the cargo workspace can produce a binary linked against libc.
The full init logic moves over from init/init.rs in the next commit."
```

---

### Task 4: Port the full init logic to libstd

Replace the stub `crates/init/src/main.rs` with the full init implementation, adapted from the current `init/init.rs` for libstd: drop `#![no_std]` and `#![no_main]`, use a standard Rust `fn main()`, build argv from `std::env::args_os()`, import all FFI from `libc::*`, replace `env!("LIBEXECDIR")` with a hardcoded path, and replace `env!("HURD_VERSION")` with `env!("CARGO_PKG_VERSION")`.

**Files:**
- Modify: `crates/init/src/main.rs`

- [ ] **Step 1: Replace `crates/init/src/main.rs` with the full port**

Overwrite `crates/init/src/main.rs` with:

```rust
// A minimalist init for the Hurd, ported from init.c via the previous
// no_std init.rs.
//
// Copyright (C) 2013, 2014 Free Software Foundation, Inc.
// This file is part of the GNU Hurd.
//
// The GNU Hurd is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2, or (at your option)
// any later version.
//
// The GNU Hurd is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// Behavior is a 1:1 port: same argp options, same signal mask, same
// fork+execv of /usr/lib/hurd/runsystem.hurd, same select-forever main
// loop, same SIGCHLD reaper.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use std::env;
use std::ptr::{null, null_mut};

use libc::{
    argp_option, argp_parse, argp_t, asprintf, c_char, c_int, c_void,
    error, errno, error_t, execv, fork, free, getpid, pid_t, select,
    sigaction, sigaction_t, sigemptyset, strdup, strsignal, waitpid,
    ARGP_ERR_UNKNOWN, SA_RESTART, SIGCHLD, SIGHUP, SIGINT, SIGQUIT, SIGTERM,
    SIGTSTP, SIGUSR1, SIGUSR2, SIG_IGN,
    WAIT_ANY, WEXITSTATUS, WIFSIGNALED, WIFSTOPPED, WNOHANG, WTERMSIG, WUNTRACED,
};

// Path to runsystem.hurd. Debian's Hurd packaging installs runsystem at
// /usr/lib/hurd/runsystem.hurd via --libexecdir=/usr/lib/hurd. Hardcoded
// here because cargo is intentionally isolated from autoconf's libexecdir
// substitution.
const RUNSYSTEM_PATH: &[u8] = b"/usr/lib/hurd/runsystem.hurd\0";

const HURD_VERSION_STR: &str =
    concat!("init (GNU Hurd) ", env!("CARGO_PKG_VERSION"), "\0");

// ---- argp wiring ----

// argp_program_version is read by glibc by name when handling --version.
// Wrap the raw pointer in a transparent Sync newtype so Rust accepts it
// as a static.
#[repr(transparent)]
pub struct CCharPtr(*const c_char);
unsafe impl Sync for CCharPtr {}

#[no_mangle]
pub static argp_program_version: CCharPtr =
    CCharPtr(HURD_VERSION_STR.as_ptr() as *const c_char);

#[repr(transparent)]
struct ArgpOpt(argp_option);
unsafe impl Sync for ArgpOpt {}

static OPTIONS: [ArgpOpt; 3] = [
    ArgpOpt(argp_option {
        name:  b"single-user\0".as_ptr() as *const c_char,
        key:   b's' as c_int,
        arg:   null(),
        flags: 0,
        // XXX: currently, -s does nothing (matches the C version).
        doc:   b"Startup system in single-user mode\0".as_ptr() as *const c_char,
        group: 0,
    }),
    ArgpOpt(argp_option {
        name:  null(),
        key:   b'a' as c_int,
        arg:   null(),
        flags: 0,
        doc:   b"Ignored for compatibility with sysvinit\0".as_ptr() as *const c_char,
        group: 0,
    }),
    ArgpOpt(argp_option {
        name: null(), key: 0, arg: null(), flags: 0, doc: null(), group: 0,
    }),
];

const DOC: &[u8] = b"A minimalist init for the Hurd\0";

const KEY_S: c_int = b's' as c_int;
const KEY_A: c_int = b'a' as c_int;

// ---- shared state ----
//
// CHILD_PID is read/written from both main() and the SIGCHLD handler —
// same race as the C version. SINGLE is set before the handler is installed
// so the race is single-threaded.

static mut CHILD_PID: pid_t = 0;
static mut SINGLE:    c_int = 0;

// ---- argp parser callback ----

unsafe extern "C" fn parse_opt(key: c_int, _arg: *mut c_char,
                               _state: *mut c_void) -> error_t {
    match key {
        KEY_S => { SINGLE = 1; 0 }
        KEY_A => 0,
        _     => ARGP_ERR_UNKNOWN,
    }
}

// ---- SIGCHLD handler ----

unsafe extern "C" fn sigchld_handler(_sig: c_int) {
    let mut status: c_int = 0;
    loop {
        let pid = waitpid(WAIT_ANY, &mut status, WNOHANG | WUNTRACED);
        if pid <= 0 {
            break;
        }

        // Since we are init, orphaned processes get reparented to us and
        // alas, all our adopted children eventually die.  Woe is us.  We
        // just need to reap the zombies to relieve the proc server of
        // its burden, and then we can forget about the little varmints.

        if pid == CHILD_PID {
            CHILD_PID = -1;

            let mut desc: *mut c_char = null_mut();
            let err: c_int;

            if WIFSIGNALED(status) {
                err = asprintf(&mut desc,
                    b"terminated abnormally (%s)\0".as_ptr() as *const c_char,
                    strsignal(WTERMSIG(status)));
            } else if WIFSTOPPED(status) {
                err = asprintf(&mut desc,
                    b"stopped abnormally (%s)\0".as_ptr() as *const c_char,
                    strsignal(WTERMSIG(status)));
            } else if WEXITSTATUS(status) == 0 {
                desc = strdup(b"finished\0".as_ptr() as *const c_char);
                err  = if desc.is_null() { -1 } else { 0 };
            } else {
                err = asprintf(&mut desc,
                    b"exited with status %d\0".as_ptr() as *const c_char,
                    WEXITSTATUS(status));
            }

            if err == -1 {
                error(0, 0,
                    b"couldn't allocate exit reason message\0".as_ptr()
                        as *const c_char);
            } else {
                error(0, 0,
                    b"child %s\0".as_ptr() as *const c_char,
                    desc);
                free(desc as *mut c_void);
            }

            // XXX: launch emergency shell.
            error(23, 0, b"panic!!\0".as_ptr() as *const c_char);
        }
    }
}

// ---- entry point ----
//
// We use a standard libstd fn main() and construct (argc, argv) from
// std::env::args_os() so we can hand them to glibc's argp_parse().
// glibc's argp may permute argv (it reorders options before positionals)
// but does not mutate the underlying strings, so the storage we own is
// safe for the duration of the call.

fn main() {
    // Materialize each argv element as a NUL-terminated heap-owned Vec<u8>
    // so the buffers are writable (argp_parse takes *mut c_char even
    // though it does not write into them in practice).
    let mut argv_bufs: Vec<Vec<u8>> = env::args_os()
        .map(|s| {
            let mut bytes = s.into_string()
                .unwrap_or_else(|os| os.to_string_lossy().into_owned())
                .into_bytes();
            bytes.push(0);
            bytes
        })
        .collect();

    let mut argv_ptrs: Vec<*mut c_char> = argv_bufs
        .iter_mut()
        .map(|v| v.as_mut_ptr() as *mut c_char)
        .collect();
    argv_ptrs.push(null_mut()); // argp_parse expects a NULL terminator

    let argc = argv_bufs.len() as c_int;
    let argv = argv_ptrs.as_mut_ptr();

    let argp = argp_t {
        options:     &OPTIONS[0].0 as *const argp_option,
        parser:      Some(parse_opt),
        args_doc:    null(),
        doc:         DOC.as_ptr() as *const c_char,
        children:    null(),
        help_filter: null(),
        argp_domain: null(),
    };

    unsafe {
        argp_parse(&argp, argc, argv, 0, null_mut(), null_mut());

        if getpid() != 1 {
            error(1, 0,
                  b"can only be run as PID 1\0".as_ptr() as *const c_char);
        }

        let mut sa = sigaction_t { sa_handler: SIG_IGN, sa_mask: 0, sa_flags: 0 };
        sigemptyset(&mut sa.sa_mask);

        sigaction(SIGHUP,  &sa, null_mut());
        sigaction(SIGINT,  &sa, null_mut());
        sigaction(SIGQUIT, &sa, null_mut());
        sigaction(SIGTERM, &sa, null_mut());
        sigaction(SIGUSR1, &sa, null_mut());
        sigaction(SIGUSR2, &sa, null_mut());
        sigaction(SIGTSTP, &sa, null_mut());

        sa.sa_handler = sigchld_handler as *const () as usize;
        sa.sa_flags  |= SA_RESTART;
        sigaction(SIGCHLD, &sa, null_mut());

        let path = RUNSYSTEM_PATH.as_ptr() as *const c_char;
        let exec_args: [*const c_char; 2] = [path, null()];

        let pid = fork();
        CHILD_PID = pid;
        match pid {
            -1 => {
                error(1, errno(),
                      b"failed to fork\0".as_ptr() as *const c_char);
            }
            0 => {
                execv(path, exec_args.as_ptr());
                error(2, errno(),
                      b"failed to execv child %s\0".as_ptr() as *const c_char,
                      path);
            }
            _ => {}
        }

        select(0, null_mut(), null_mut(), null_mut(), null_mut());
        // Not reached.
    }

    // Keep argv_bufs alive until here so glibc's argp_parse never sees
    // dangling pointers. (Vec drops at end of scope.)
    drop(argv_bufs);
    drop(argv_ptrs);
}
```

Key changes from the previous `init/init.rs`:

| Aspect | Before (no_std + no_main) | After (libstd) |
|---|---|---|
| Attributes | `#![no_std] #![no_main]` | none (libstd default) |
| Entry point | `extern "C" fn main(argc, argv)` | standard `fn main()` |
| Argv source | Provided by crt1 / glibc directly | Built from `std::env::args_os()` |
| FFI types | Inlined in source | Imported from `libc` crate |
| `LIBEXECDIR` | `env!("LIBEXECDIR")` set by Makefile | Hardcoded `/usr/lib/hurd/` |
| Version string | `env!("HURD_VERSION")` from Makefile | `env!("CARGO_PKG_VERSION")` |
| Panic handler | Custom `#[panic_handler]` calling `abort` | libstd default + `panic = "abort"` in profile |

- [ ] **Step 2: Build the binary**

```sh
cargo build --release
```

Expected: clean build to `target/release/init`. The first build of `init` after this change may take longer because libstd is being linked in for the first time in this workspace.

If you see `error[E0432]: unresolved import \`libc::...\``: a name in the `use libc::{...}` block doesn't match what `crates/libc/src/lib.rs` exports. Compare names character-for-character.

If you see linker errors about `__rust_alloc` or `__rust_dealloc`: this means libstd's allocator isn't being linked. This should not happen with a normal libstd build — if it does, run `cargo clean && cargo build --release` to force a rebuild from scratch.

- [ ] **Step 3: Verify the binary is sane**

```sh
file target/release/init
```

Expected: `ELF 64-bit LSB executable, x86-64, version 1 (GNU/kFreeBSD), dynamically linked, interpreter /lib/ld.so, for GNU/Hurd, ...`. The "GNU/kFreeBSD" tag in the ELF OSABI field is normal — Hurd reuses that ABI byte.

```sh
target/release/init --version
```

Expected output: `init (GNU Hurd) 0.9.0` followed by a newline. argp prints this from the `argp_program_version` static.

```sh
target/release/init --help
```

Expected: argp prints help including the `-s, --single-user` and `-a` options and the DOC string.

If `--version` or `--help` hang or segfault, argp_parse is failing — verify the OPTIONS array is correctly null-terminated and that the `argp_t` fields you pass match the layout in `crates/libc/src/lib.rs`.

- [ ] **Step 4: Commit**

```sh
git add crates/init/src/main.rs
git commit -m "feat(init): port to libstd-based cargo build

Replaces the no_std + no_main rustc-direct init with a normal libstd
binary built by cargo. Behavior is unchanged from the prior init.rs
and from the original init.c: same argp options, same signal mask,
same fork+execv of /usr/lib/hurd/runsystem.hurd, same SIGCHLD reaper,
same select-forever main loop.

Argv is materialized from std::env::args_os() into heap-owned NUL-
terminated buffers handed to argp_parse(). Version string is sourced
from CARGO_PKG_VERSION via env!() now that there is no Makefile to
inject HURD_VERSION.

Build via: cargo build --release"
```

---

### Task 5: Symbol and dependency audit

Verify the cargo-built `init` binary depends on the expected shared libraries and imports the expected libc symbols. This catches problems that wouldn't manifest until the boot test (Task 6).

**Files:** None (read-only inspection of `target/release/init`).

- [ ] **Step 1: List dynamic dependencies**

```sh
objdump -p target/release/init | grep NEEDED
```

Expected (order may vary):

```
  NEEDED               libpthread.so.0.3
  NEEDED               libhurdbugaddr.so.0.3
  NEEDED               libgcc_s.so.1
  NEEDED               libc.so.0.3
```

Acceptable variations: `ld.so` may or may not appear depending on cargo's link choices; additional libs like `libm.so.0.3` may be pulled in by libstd. Anything *missing* from the list above (especially `libc.so.0.3`) is a build error.

If `libhurdbugaddr.so.0.3` is missing, the `.cargo/config.toml` `-lhurdbugaddr` rustflag did not propagate — re-verify the config file content.

- [ ] **Step 2: Confirm the libc functions init uses are imported as expected**

```sh
nm -D target/release/init | awk '$2=="U"' | sort
```

The output is the list of symbols resolved at runtime. Verify that *at minimum* these names appear:

```
__errno_location
abort
argp_parse
asprintf
error
execv
fork
free
getpid
select
sigaction
sigemptyset
strdup
strsignal
waitpid
```

Additionally, libstd-provided symbols will appear (e.g. various `__pthread_*`, `malloc`, `realloc`, etc.) — those are expected.

- [ ] **Step 3: Record the audit results**

This step is documentation, not code. In your terminal, capture:

```sh
{
  echo "=== file ==="
  file target/release/init
  echo
  echo "=== NEEDED ==="
  objdump -p target/release/init | grep NEEDED
  echo
  echo "=== sizes ==="
  size target/release/init
  ls -lh target/release/init
} | tee /tmp/init-audit-cargo.txt
```

Keep `/tmp/init-audit-cargo.txt` around — you'll reference it in the boot test commit message and as evidence in any future bug reports.

- [ ] **Step 4: No commit (this task is verification only)**

If audits passed, proceed to Task 6. If any expected NEEDED entry or imported symbol is missing, return to Task 1 or 4 to fix the configuration; do not proceed to the boot test with a suspect binary.

---

### Task 6: Boot test on the Hurd VM

Replace `/hurd/init` on the running VM with the cargo-built binary and verify the system still boots and behaves correctly. This is a manual, destructive-ish step: if the new init is broken, the VM will fail to boot and you'll need the backup to recover.

**Files:** None (only `/hurd/init` on the VM filesystem changes).

- [ ] **Step 1: Backup the current `/hurd/init`**

On the Hurd VM, as root:

```sh
cp -p /hurd/init /hurd/init.cargo-backup
ls -l /hurd/init /hurd/init.cargo-backup
```

The backup is your recovery path. Do not skip this step.

- [ ] **Step 2: Install the cargo-built binary**

```sh
install -m 0755 -o root -g root \
    target/release/init /hurd/init
ls -l /hurd/init
file /hurd/init
```

Expected: `/hurd/init` now has the size from `target/release/init` and `file` still reports a Hurd ELF executable.

- [ ] **Step 3: Sync and reboot**

```sh
sync
reboot
```

The VM will reboot. If it fails to come up: in your bootloader / qemu console, the recovery path is to boot a rescue translator and `mv /hurd/init.cargo-backup /hurd/init`. (For libstd-related failures the system may not get far enough for `/hurd/startup` to launch a usable shell — be ready to recover via the host.)

- [ ] **Step 4: After reboot, validate**

On the booted VM:

```sh
# PID 1 should be our new init.
ps -ef | head -5
```

Expected: `init [2]` (or whatever runlevel string argp sets) as PID 1, with `/hurd/startup`, `gnumach`, and `/hurd/proc` underneath as you saw before.

```sh
# Zombie-reaping smoke test.
for i in 1 2 3 4 5; do ( ( sleep 1 ; exit 0 ) & ); done
sleep 3
ps -ef | grep -E '<defunct>|Z '
```

Expected: no output. Any `<defunct>` lines indicate init is not running the SIGCHLD reaper correctly — that's a regression and must be fixed before proceeding.

```sh
# --version and --help still work if invoked manually (they don't normally
# run, but verify the argp binding survived the port).
/hurd/init --version
/hurd/init --help
```

Expected: `init (GNU Hurd) 0.9.0` and the help text.

- [ ] **Step 5: If validation passes, remove the backup**

```sh
rm /hurd/init.cargo-backup
```

If validation **failed**, leave the backup in place and revert: `cp -p /hurd/init.cargo-backup /hurd/init && reboot`. Then go back to Task 4 or 5 to diagnose.

- [ ] **Step 6: No commit (this task is operational)**

Record validation results in your notes for the final commit in Task 8.

---

### Task 7: Revert the autoconf/Make Rust additions

The recent additions to `configure.ac` and `config.make.in` for `RUSTC` and `rust_target` are now dead code under the isolated cargo model. Remove them.

**Files:**
- Modify: `configure.ac`
- Modify: `config.make.in`

- [ ] **Step 1: Revert the configure.ac additions**

In `configure.ac`, delete the block at lines 113-121:

```
# rustc is used to build the init server (init/init.rs).  We map the
# autoconf canonical $host_cpu to the corresponding Rust target triple.
AC_CHECK_TOOL(RUSTC, rustc)
case "$host_cpu" in
  i?86)    rust_target=i686-unknown-hurd-gnu ;;
  x86_64)  rust_target=x86_64-unknown-hurd-gnu ;;
  *)       rust_target= ;;
esac
AC_SUBST(rust_target)

```

After the edit, line 112 (the `fi` ending the MIG check) should be immediately followed by the `dnl Let these propagate from the environment.` line that was previously at line 123.

- [ ] **Step 2: Revert the config.make.in additions**

In `config.make.in`, delete lines 54-55:

```
RUSTC = @RUSTC@
rust_target = @rust_target@
```

The lines before (`SED = @SED@`) and after (`# Compilation flags. ...`) should now be adjacent.

- [ ] **Step 3: Regenerate configure and re-run it**

```sh
autoreconf -i
./configure
```

Expected: configure completes without errors and without mentioning rustc. The `config.make` it generates should no longer contain `RUSTC =` or `rust_target =`.

```sh
grep -E 'RUSTC|rust_target' config.make
```

Expected: no output.

- [ ] **Step 4: Smoke-test that `make` still works for a C target**

Pick any small C-only subdirectory and build it:

```sh
make -C libihash
```

Expected: clean compilation. If `make` complains about undefined RUSTC variables, you missed a reference — `grep -rn RUSTC` across the tree to find stragglers (only `Makeconf` and subdirectory Makefiles should be searched; `target/` is irrelevant).

- [ ] **Step 5: Commit**

```sh
git add configure.ac config.make.in
git commit -m "build: revert RUSTC and rust_target from autoconf/Makeconf

These were added when init was built via rustc-direct from the existing
Make system. Now that cargo owns the Rust build end-to-end via the
crates/ workspace, the variables are dead code. Remove them so there is
exactly one source of truth for Rust build configuration."
```

---

### Task 8: Delete the old init/ directory

The old `init/` directory contains `init/Makefile` (the rustc-direct recipe) and `init/init.rs` (now superseded by `crates/init/src/main.rs`). Remove the directory entirely.

**Files:**
- Delete: `init/Makefile`
- Delete: `init/init.rs`
- Delete: `init/` (directory itself)
- Modify: `Makefile` (top-level) if it lists `init` in `prog-subdirs`

- [ ] **Step 1: Confirm what's in init/**

```sh
ls -la init/
git status init/
```

Expected: only `Makefile` and `init.rs` (and `..`). `init.c` should have already been deleted in a prior commit. If you see other files (e.g. a stray `.o`, an editor swap file), investigate before deleting.

- [ ] **Step 2: Remove init/ from the top-level Makefile's prog-subdirs list**

Inspect the top-level `Makefile`:

```sh
grep -n init Makefile
```

Look for `init` (likely on a line ending with `\` continuing a list assignment to `prog-subdirs`). Remove it. The relevant section around line 45-50 currently reads:

```make
prog-subdirs = auth proc exec term \
	       ext2fs isofs tmpfs fatfs \
	       ...
	       startup \
	       init \
	       devnode \
	       ...
```

After the edit, the line containing `init \` is removed. Make sure not to break the line continuations of neighboring entries (the `startup \` and `devnode \` lines).

- [ ] **Step 3: Delete the init directory**

```sh
git rm -r init/
```

Expected: git reports two file deletions (`init/Makefile` and `init/init.rs`).

- [ ] **Step 4: Verify the top-level make still parses**

```sh
make -n all 2>&1 | head -5
```

Expected: a few lines of `make`'s dry-run output for the C subdirectories, no mention of `init`. If make says "No rule to make target 'init'", a Makefile somewhere else (probably the top-level) still references it — find and remove the reference.

- [ ] **Step 5: Final commit**

```sh
git add Makefile init/
git commit -m "build(init): remove old init/ directory

The Rust init now lives at crates/init/ and builds via cargo. The
init/Makefile + init.rs (rustc-direct) are superseded.

This completes the migration to the isolated cargo workspace. See
docs/superpowers/specs/2026-05-13-cargo-rust-build-design.md for the
approved design and docs/superpowers/plans/2026-05-13-cargo-rust-build.md
for the executed plan.

Build instructions for Rust components:
  cargo build --release
  sudo cp target/release/init /hurd/init

C components continue to build as before:
  ./configure && make"
```

---

## Self-review

Spec coverage check, walking the spec's sections:

- **Architecture & directory layout** → Task 1 creates `Cargo.toml`, `.cargo/config.toml`, `rust-toolchain.toml`, `.gitignore` update. Tasks 3-4 create `crates/init/`. Task 2 populates `crates/libc/`. ✓
- **Components / Workspace root Cargo.toml** → Task 1 step 1. ✓
- **Components / .cargo/config.toml** → Task 1 step 2. ✓
- **Components / libc crate** → Task 1 steps 4-5 (skeleton), Task 2 step 1 (populated). ✓
- **Components / init crate** → Task 3 (skeleton), Task 4 (full port). ✓
- **Build pipeline / cargo build flow** → Tasks 1, 2, 3, 4 each end with `cargo build`. ✓
- **Build pipeline / install flow** → Task 6 step 2 (`install -m 0755 ...`). ✓
- **What gets reverted** → Task 7 (configure.ac + config.make.in), Task 8 (init/ directory). ✓
- **Failure modes** → addressed inline in Task 1 step 7, Task 3 step 3, Task 4 step 2 with specific diagnostic guidance. ✓
- **Verification** → Task 5 (symbol audit), Task 6 step 4 (boot + zombie-reaping). ✓
- **Migration order** → Task numbering matches the spec's 8-step migration order exactly. ✓

Placeholder scan: no "TBD", "TODO", "implement later", or "fill in details" patterns. Every code block contains complete, runnable content.

Type consistency check: function names used in `crates/init/src/main.rs` (Task 4) match exactly what `crates/libc/src/lib.rs` (Task 2) exports — verified `argp_parse`, `sigaction`, `sigemptyset`, `fork`, `execv`, `waitpid`, `select`, `error`, `errno`, `strsignal`, `asprintf`, `strdup`, `free`, `getpid`, `__errno_location`, `abort`, `WIFSIGNALED`, `WIFSTOPPED`, `WEXITSTATUS`, `WTERMSIG`, `WAIT_ANY`, `WNOHANG`, `WUNTRACED`, `SIG_IGN`, `SA_RESTART`, `SIGHUP`/`SIGINT`/`SIGQUIT`/`SIGTERM`/`SIGUSR1`/`SIGUSR2`/`SIGTSTP`/`SIGCHLD`, `ARGP_ERR_UNKNOWN`, `argp_option`, `argp_t`, `sigaction_t`, `c_char`, `c_int`, `c_void`, `error_t`, `pid_t`.

Scope check: single feature (migrate Rust build into isolated cargo workspace), single binary output (init), no surprise additional components.
