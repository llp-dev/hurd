//! Pure-Rust reimplementation of libtrivfs.
//!
//! The C `libtrivfs` is ~80 small files of MIG handlers + a handful of
//! files with real logic (`startup.c`, `cntl-create.c`, `demuxer.c`,
//! `fsys-getroot.c`). This crate ships C-ABI-compatible replacements
//! for the public surface (the `trivfs_*` symbols), keeping the
//! underlying port-management to the C `libports` (via `ports-sys`).
//!
//! ## What's done
//!
//! - `trivfs_startup`: full bootstrap rendezvous, including a hand-rolled
//!   `fsys_startup` MIG outbound (port-transferring, old-format).
//! - `trivfs_add_port_bucket` / `trivfs_add_control_port_class` /
//!   `trivfs_add_protid_port_class`: thin wrappers around `ports_create_*`.
//! - `trivfs_clean_cntl` / `trivfs_clean_protid`: libports clean callbacks.
//! - C-ABI-compatible `TrivfsControl`, `TrivfsProtid`, `TrivfsPeropen`,
//!   `port_info` layouts, with compile-time size assertions.
//!
//! ## What's missing
//!
//! - `trivfs_demuxer`: stub. Returns 0 for every msgh_id; effective for
//!   bootstrap but means real client RPCs (`stat`, `open`, etc.) will
//!   bounce with MIG_BAD_ID until handlers are filled in.
//! - `trivfs_create_control`, `trivfs_make_node`, `trivfs_make_peropen`,
//!   `trivfs_open`, `trivfs_protid_dup`, `trivfs_set_options`,
//!   `trivfs_append_args`: not implemented.
//! - Hooks (`trivfs_check_open_hook`, `trivfs_open_hook`, etc.) and the
//!   `trivfs_runtime_argp` mechanism: not bound.
//!
//! ## Linking model
//!
//! To use this crate, a translator depends on it from Cargo.toml (no
//! `links = "trivfs"` anywhere). The translator binary provides strong
//! definitions for the `trivfs_*` user globals and callbacks via
//! `#[no_mangle]`; this crate references them through `extern "C"`
//! blocks below. The linker resolves both sides in the final binary.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

mod cleanup;
mod demuxer;
mod startup;
pub mod types;

// Re-export the public C-ABI surface so consumers can also reach the
// symbols through the Rust API (`trivfs::trivfs_startup(...)`) instead
// of having to declare extern blocks themselves.
pub use cleanup::{trivfs_clean_cntl, trivfs_clean_protid};
pub use demuxer::trivfs_demuxer;
pub use startup::{
    trivfs_add_control_port_class, trivfs_add_port_bucket,
    trivfs_add_protid_port_class, trivfs_startup,
};
pub use types::{port_info, TrivfsControl, TrivfsPeropen, TrivfsProtid};

// Convenience aliases matching the legacy C type names so translator
// code can stay close to the C API surface.
pub type trivfs_control = TrivfsControl;
pub type trivfs_protid  = TrivfsProtid;
pub type trivfs_peropen = TrivfsPeropen;
pub type trivfs_control_t = *mut TrivfsControl;
pub type trivfs_protid_t  = *mut TrivfsProtid;

// ---- constants the user might want (re-exports) ----

use libc::c_int;

/// `FSTYPE_MISC` from `<hurd/hurd_types.h>` — the catch-all "this
/// translator doesn't fit any real fs type" tag, used by translators
/// like shutdown that don't present a filesystem at all.
pub const FSTYPE_MISC:       c_int = 0x16;
pub const FSYS_GOAWAY_FORCE: c_int = 0x04;

/// POSIX open-flag bits as Hurd glibc uses them; only the ones
/// translators typically declare in `trivfs_allow_open`.
pub const O_READ:  c_int = 1;
pub const O_WRITE: c_int = 2;

// ---- globals the user MUST define ----
//
// The translator binary provides strong definitions; we reference them
// here so handlers (when we have them) can read e.g. `trivfs_fstype`
// during io_stat / fsys_getroot.

extern "C" {
    pub static mut trivfs_fstype:         c_int;
    pub static mut trivfs_fsid:           c_int;
    pub static mut trivfs_support_read:   c_int;
    pub static mut trivfs_support_write:  c_int;
    pub static mut trivfs_support_exec:   c_int;
    pub static mut trivfs_allow_open:     c_int;
}

// ---- callbacks the user MUST define ----
//
// `trivfs_modify_stat` is called from io_stat handler (not yet wired)
// to let the user adjust the stat block before reply.
// `trivfs_goaway` is called from fsys_goaway handler (not yet wired)
// to ask the translator to exit cleanly.

extern "C" {
    pub fn trivfs_modify_stat(cred: *mut TrivfsProtid, st: *mut core::ffi::c_void);
    pub fn trivfs_goaway(fsys: *mut TrivfsControl, flags: c_int) -> mach_sys::kern_return_t;
}
