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
