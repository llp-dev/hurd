// A minimalist init for the Hurd, ported from init.c.
//
// Copyright (C) 2013,14 Free Software Foundation, Inc.
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
// Behaviorally a 1:1 port: same argp options, same signal mask, same
// fork+execv of ${libexecdir}/runsystem.hurd, same select-forever.
// Uses #![no_std] + #![no_main] and calls glibc directly via extern "C",
// so the final link is identical to the original C version.

#![no_std]
#![no_main]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use core::ptr::{null, null_mut};

// -------- libc types (Hurd glibc layout) --------

type pid_t   = c_int;
type error_t = c_int;
// Hurd glibc declares sigset_t as `unsigned long int`.
type sigset_t = c_ulong;

#[repr(C)]
struct sigaction_t {
    // Holds either a function pointer or SIG_IGN/SIG_DFL. Glibc treats this
    // slot as a union of (void (*)(int)) and (void (*)(int, siginfo_t*, void*));
    // ABI-wise it's just one pointer-sized word.
    sa_handler: usize,
    sa_mask:    sigset_t,
    sa_flags:   c_int,
    // No sa_restorer on the Hurd — signal trampolining is done in userspace.
}

#[repr(C)]
struct argp_option {
    name:  *const c_char,
    key:   c_int,
    arg:   *const c_char,
    flags: c_int,
    doc:   *const c_char,
    group: c_int,
}

type argp_parser_t =
    Option<unsafe extern "C" fn(c_int, *mut c_char, *mut c_void) -> error_t>;

#[repr(C)]
struct argp_t {
    options:     *const argp_option,
    parser:      argp_parser_t,
    args_doc:    *const c_char,
    doc:         *const c_char,
    children:    *const c_void,
    help_filter: *const c_void,
    argp_domain: *const c_char,
}

// -------- libc imports --------

extern "C" {
    fn argp_parse(argp: *const argp_t, argc: c_int, argv: *mut *mut c_char,
                  flags: c_uint, arg_index: *mut c_int,
                  input: *mut c_void) -> error_t;

    fn getpid() -> pid_t;
    fn fork()   -> pid_t;
    fn execv(path: *const c_char, argv: *const *const c_char) -> c_int;
    fn waitpid(pid: pid_t, wstatus: *mut c_int, options: c_int) -> pid_t;
    fn select(nfds: c_int,
              readfds:  *mut c_void,
              writefds: *mut c_void,
              exceptfds:*mut c_void,
              timeout:  *mut c_void) -> c_int;

    fn sigaction(signum: c_int, act: *const sigaction_t,
                 oldact: *mut sigaction_t) -> c_int;
    fn sigemptyset(set: *mut sigset_t) -> c_int;

    fn error(status: c_int, errnum: c_int, format: *const c_char, ...);
    fn strsignal(sig: c_int) -> *const c_char;
    fn asprintf(strp: *mut *mut c_char, fmt: *const c_char, ...) -> c_int;
    fn strdup(s: *const c_char) -> *mut c_char;
    fn free(p: *mut c_void);

    // Glibc's errno is thread-local; access it through __errno_location().
    fn __errno_location() -> *mut c_int;

    fn abort() -> !;
}

#[inline] fn errno() -> c_int { unsafe { *__errno_location() } }

// -------- Hurd-specific constants --------
//
// Signal numbers follow BSD numbering on the Hurd, not Linux:
//   sysdeps/mach/hurd/bits/signum-arch.h in glibc.
const SIGHUP:  c_int =  1;
const SIGINT:  c_int =  2;
const SIGQUIT: c_int =  3;
const SIGTERM: c_int = 15;
const SIGTSTP: c_int = 18;
const SIGCHLD: c_int = 20;
const SIGUSR1: c_int = 30;
const SIGUSR2: c_int = 31;

const SIG_IGN:    usize = 1;
const SA_RESTART: c_int = 0x0002;

const WAIT_ANY:  pid_t = -1;
const WNOHANG:   c_int = 1;
const WUNTRACED: c_int = 2;

// POSIX status-word decoding. Hurd glibc uses the same encoding.
#[inline] fn WTERMSIG(s: c_int)    -> c_int { s & 0x7f }
#[inline] fn WIFSIGNALED(s: c_int) -> bool  { (((s & 0x7f) + 1) >> 1) > 0 }
#[inline] fn WIFSTOPPED(s: c_int)  -> bool  { (s & 0xff) == 0x7f }
#[inline] fn WEXITSTATUS(s: c_int) -> c_int { (s >> 8) & 0xff }

// ARGP_ERR_UNKNOWN == E2BIG. On Hurd, E2BIG = _HURD_ERRNO(7) = 0x40000007
// because errno values are tagged with a sub-system code so the same int
// can carry POSIX and Mach error codes.
const ARGP_ERR_UNKNOWN: error_t = 0x40000007;

// -------- Shared state --------
//
// child_pid is read/written from both main() and the SIGCHLD handler — same
// race as the C version. SINGLE is set before the handler is installed.

static mut CHILD_PID: pid_t = 0;
static mut SINGLE:    c_int = 0;

// -------- argp wiring --------
//
// `argp_program_version` must be a C-visible global of type `const char *`;
// glibc reads it by name when handling --version. We wrap the pointer in a
// #[repr(transparent)] Sync-able newtype so Rust accepts it as a static.

#[repr(transparent)]
pub struct CCharPtr(*const c_char);
unsafe impl Sync for CCharPtr {}

const VERSION_CSTR: &str =
    concat!("init (GNU Hurd) ", env!("HURD_VERSION"), "\0");

#[no_mangle]
pub static argp_program_version: CCharPtr =
    CCharPtr(VERSION_CSTR.as_ptr() as *const c_char);

#[repr(transparent)]
struct ArgpOpt(argp_option);
unsafe impl Sync for ArgpOpt {}

static OPTIONS: [ArgpOpt; 3] = [
    ArgpOpt(argp_option {
        name:  b"single-user\0".as_ptr() as *const c_char,
        key:   b's' as c_int,
        arg:   null(),
        flags: 0,
        // XXX: Currently, -s does nothing (matches the C version).
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

unsafe extern "C" fn parse_opt(key: c_int, _arg: *mut c_char,
                               _state: *mut c_void) -> error_t {
    match key {
        KEY_S => { SINGLE = 1; 0 }
        KEY_A => 0,             // Ignored.
        _     => ARGP_ERR_UNKNOWN,
    }
}

// -------- SIGCHLD handler --------

unsafe extern "C" fn sigchld_handler(_sig: c_int) {
    // A child died. Find its status.
    let mut status: c_int = 0;
    loop {
        let pid = waitpid(WAIT_ANY, &mut status, WNOHANG | WUNTRACED);
        if pid <= 0 {
            // No more children.
            break;
        }

        // Since we are init, orphaned processes get reparented to us and
        // alas, all our adopted children eventually die.  Woe is us.  We
        // just need to reap the zombies to relieve the proc server of
        // its burden, and then we can forget about the little varmints.

        if pid == CHILD_PID {
            // The big magilla bit the dust.
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

// -------- entry point --------
//
// Glibc's __libc_start_main calls our `main(argc, argv)` after CRT setup,
// exactly as it does for the C version. We use #![no_main] so we provide
// the `main` symbol ourselves; crt1/crti come from libc as usual via the
// link recipe in Makeconf.

#[no_mangle]
pub unsafe extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    let argp = argp_t {
        options:     &OPTIONS[0].0 as *const argp_option,
        parser:      Some(parse_opt),
        args_doc:    null(),
        doc:         DOC.as_ptr() as *const c_char,
        children:    null(),
        help_filter: null(),
        argp_domain: null(),
    };
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

    const RUNSYSTEM: &str = concat!(env!("LIBEXECDIR"), "/runsystem.hurd\0");
    let path = RUNSYSTEM.as_ptr() as *const c_char;
    let args: [*const c_char; 2] = [path, null()];

    let pid = fork();
    CHILD_PID = pid;
    match pid {
        -1 => {
            error(1, errno(),
                  b"failed to fork\0".as_ptr() as *const c_char);
        }
        0 => {
            execv(path, args.as_ptr());
            error(2, errno(),
                  b"failed to execv child %s\0".as_ptr() as *const c_char,
                  path);
        }
        _ => {}
    }

    select(0, null_mut(), null_mut(), null_mut(), null_mut());
    // Not reached.
    0
}

// -------- panic handler --------
//
// init never panics intentionally; if it ever did, aborting is the most
// honest outcome (a Rust panic in PID 1 is no worse than a crash in PID 1).

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { abort() }
}
