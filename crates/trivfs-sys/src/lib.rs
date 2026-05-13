//! FFI bindings to Hurd's libtrivfs.
//!
//! libtrivfs is the "trivial filesystem" library used by translators that
//! do not present a meaningful filesystem — like /servers/shutdown (we
//! only want to be invokable for the shutdown RPC) or /servers/password.
//!
//! Only the surface current Rust consumers (shutdown) need is bound. Many
//! callback hooks and helper functions from <hurd/trivfs.h> are omitted
//! and can be added when a future server needs them.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use core::marker::PhantomData;
use libc::{c_int, error_t};
use mach_sys::{mach_msg_header_t, mach_port_t};
use ports_sys::{port_bucket, port_class};

// ---- opaque types ----
//
// struct trivfs_control and struct trivfs_protid have public fields in
// libtrivfs's header, but consumers never need to inspect those fields
// from Rust — every operation goes through libtrivfs functions. So we
// keep them opaque (zero-size + PhantomData) on the Rust side.

#[repr(C)]
pub struct trivfs_control {
    _opaque:  [u8; 0],
    _phantom: PhantomData<(*mut u8, ::core::marker::PhantomPinned)>,
}

#[repr(C)]
pub struct trivfs_protid {
    _opaque:  [u8; 0],
    _phantom: PhantomData<(*mut u8, ::core::marker::PhantomPinned)>,
}

pub type trivfs_protid_t = *mut trivfs_protid;
pub type trivfs_control_t = *mut trivfs_control;

// ---- globals the user must define / may write ----
//
// libtrivfs declares these as `extern int trivfs_fstype;` etc. The actual
// strong definitions are provided by the USER (the translator binary).
// For shutdown, we write them in the shutdown crate, not here.

extern "C" {
    pub static mut trivfs_fstype:         c_int;
    pub static mut trivfs_fsid:           c_int;
    pub static mut trivfs_support_read:   c_int;
    pub static mut trivfs_support_write:  c_int;
    pub static mut trivfs_support_exec:   c_int;
    pub static mut trivfs_allow_open:     c_int;
}

// ---- constants ----
//
// From <hurd/hurd_types.h>:

pub const FSTYPE_MISC:        c_int = 0x16;
pub const FSYS_GOAWAY_FORCE:  c_int = 0x04;

// POSIX open flags (sys/file.h); only the ones shutdown uses.
pub const O_READ:  c_int = 1;
pub const O_WRITE: c_int = 2;

// ---- extern functions ----

#[link(name = "trivfs")]
extern "C" {
    /// Allocate (or reuse) a control port class and add it to libtrivfs's
    /// known classes. Pass `&mut null_mut()` to ask libtrivfs to allocate
    /// a new class. After return, the class is owned by libtrivfs.
    pub fn trivfs_add_control_port_class(class: *mut *mut port_class) -> error_t;

    /// Same idea, for the protid port class.
    pub fn trivfs_add_protid_port_class(class: *mut *mut port_class) -> error_t;

    /// Allocate (or reuse) a port bucket. Pass `&mut null_mut()` to ask
    /// libtrivfs to allocate.
    pub fn trivfs_add_port_bucket(bucket: *mut *mut port_bucket) -> error_t;

    /// Hand the parent translator our control port and complete the
    /// startup handshake (fsys_startup). On return, *control points to
    /// a newly-created trivfs_control.
    pub fn trivfs_startup(
        bootstrap:      mach_port_t,
        flags:          c_int,
        control_class:  *mut port_class,
        control_bucket: *mut port_bucket,
        protid_class:   *mut port_class,
        protid_bucket:  *mut port_bucket,
        control:        *mut *mut trivfs_control,
    ) -> error_t;

    /// Default demuxer for trivfs ports — dispatches the fs.defs / io.defs
    /// RPCs (open, read, stat, close, ...) to libtrivfs internal handlers.
    /// Returns nonzero if the message was handled.
    pub fn trivfs_demuxer(
        inp:  *mut mach_msg_header_t,
        outp: *mut mach_msg_header_t,
    ) -> c_int;
}
