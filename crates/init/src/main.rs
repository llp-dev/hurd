// A minimalist init for the Hurd. no_std + no_main + cargo.
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
// Behavior is a 1:1 port of init.c: same argp options, same signal mask,
// same fork+execv of /usr/lib/hurd/runsystem.hurd, same select-forever
// main loop, same SIGCHLD reaper.
//
// Built as no_std + no_main: glibc's crt1 provides _start, _start calls
// our `extern "C" fn main(argc, argv)`. There is no libstd runtime, no
// allocator, no panic-unwinding. The binary is tens of KB, not megabytes.

#![no_std]
#![no_main]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use core::ptr::{null, null_mut};

use libc::{
    argp_option, argp_parse, argp_t, asprintf, c_char, c_int, c_void,
    error, errno, error_t, execv, fork, free, getpid, pid_t, select,
    sigaction, sigaction_t, sigemptyset, strdup, strsignal, waitpid,
    ARGP_ERR_UNKNOWN, SA_RESTART, SIGCHLD, SIGHUP, SIGINT, SIGQUIT, SIGTERM,
    SIGTSTP, SIGUSR1, SIGUSR2, SIG_IGN,
    WAIT_ANY, WEXITSTATUS, WIFSIGNALED, WIFSTOPPED, WNOHANG, WTERMSIG, WUNTRACED,
};

// hurd_rt provides the no_main + panic_handler boilerplate. We just call
// hurd_rt::main!(|argc, argv| { ... }) below instead of declaring our own
// extern "C" fn main.

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
// same race as the C version. SINGLE is set before the handler is
// installed so the race is single-threaded.

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
// #[hurd_rt::entry] expands to #[no_mangle] pub unsafe extern "C" fn main,
// the symbol crt1 invokes. argc/argv come directly from the kernel via
// crt1 — no libstd argv-marshalling needed. The panic_handler comes from
// hurd_rt too, so this file doesn't repeat the boilerplate.

#[hurd_rt::entry]
fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
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
    0  // not reached
}
