//! FFI bindings to Hurd's libports.
//!
//! libports is the Hurd library that implements Mach-port lifecycle
//! management for servers: port classes, port buckets, the multithreaded
//! message-handling loop, RPC inhibition during shutdown, etc.
//!
//! Only the symbols current Rust consumers need are bound; grow as more
//! servers come online.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use core::marker::PhantomData;
use libc::{c_int, c_void, error_t};
use mach_sys::{mach_msg_header_t, mach_port_t};

// ---- opaque types ----
//
// `struct port_class` and `struct port_bucket` are internal to libports.
// Consumers only ever see pointers to them, never construct or inspect
// the contents. The zero-size array + PhantomData<*mut u8> pattern makes
// the types unconstructible in safe Rust and not Send/Sync by default.

#[repr(C)]
pub struct port_class {
    _opaque:  [u8; 0],
    _phantom: PhantomData<(*mut u8, ::core::marker::PhantomPinned)>,
}

#[repr(C)]
pub struct port_bucket {
    _opaque:  [u8; 0],
    _phantom: PhantomData<(*mut u8, ::core::marker::PhantomPinned)>,
}

// ---- function-pointer typedefs ----

/// libports demuxer signature: given an incoming message header, examine
/// `msgh_id`, dispatch to the matching handler, and fill `outp` with the
/// reply. Returns nonzero if the message was handled, 0 otherwise.
pub type ports_demuxer_type = unsafe extern "C" fn(
    inp:  *mut mach_msg_header_t,
    outp: *mut mach_msg_header_t,
) -> c_int;

/// "Hook" callback type for `ports_manage_port_operations_multithread`.
/// Called periodically by libports' worker threads; shutdown.c passes 0
/// (no hook).
pub type ports_hook_type = Option<unsafe extern "C" fn()>;

// ---- extern functions ----

#[link(name = "ports")]
extern "C" {
    /// Drive the message-handling loop on `bucket` with the given demuxer.
    /// Spawns worker threads internally and never returns under normal
    /// operation. Returns when the bucket is shut down.
    ///
    /// `global_timeout` and `local_timeout` are in milliseconds; controls
    /// how long worker threads stay alive after going idle.
    pub fn ports_manage_port_operations_multithread(
        bucket:         *mut port_bucket,
        demuxer:        ports_demuxer_type,
        global_timeout: c_int,
        local_timeout:  c_int,
        hook:           ports_hook_type,
    );

    /// Block new RPCs from arriving for ports in `class`. Used during
    /// orderly shutdown of a translator.
    pub fn ports_inhibit_class_rpcs(class: *mut port_class) -> error_t;

    /// Reverse of ports_inhibit_class_rpcs — allow new RPCs again.
    pub fn ports_resume_class_rpcs(class: *mut port_class);

    /// Enable a class to accept new ports.
    pub fn ports_enable_class(class: *mut port_class);

    /// Number of extant ports in `class`. Used by trivfs_goaway to decide
    /// whether unmount is safe.
    pub fn ports_count_class(class: *mut port_class) -> c_int;

    /// Allocate a fresh bucket; returns NULL on OOM.
    pub fn ports_create_bucket() -> *mut port_bucket;

    /// Allocate a fresh port class. `clean_routine` is invoked on each
    /// port in this class when it is destroyed; `dropweak_routine` is
    /// invoked when a weak reference should be dropped (libtrivfs
    /// doesn't use weak references so we pass NULL).
    pub fn ports_create_class(
        clean_routine:    Option<unsafe extern "C" fn(*mut c_void)>,
        dropweak_routine: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> *mut port_class;

    /// Create a fresh port in `class`/`bucket` with `size` bytes of
    /// memory. The returned pointer's first `sizeof(port_info)` bytes
    /// are the libports header; the caller's struct should begin with a
    /// `struct port_info` and reserve `size` total bytes including its
    /// trailing private data.
    pub fn ports_create_port(
        class:   *mut port_class,
        bucket:  *mut port_bucket,
        size:    usize,
        result:  *mut *mut c_void,
    ) -> error_t;

    /// Return the receive-right name associated with a port. The caller
    /// is then responsible for synthesising a send-right from it (or use
    /// `ports_get_send_right` which does both).
    pub fn ports_get_right(port: *mut c_void) -> mach_port_t;

    /// Return a send-right name suitable for handing out to a peer.
    /// Libports also arranges for dead-name notifications etc.
    pub fn ports_get_send_right(port: *mut c_void) -> mach_port_t;

    /// Increment the (hard) reference count on a port.
    pub fn ports_port_ref(port: *mut c_void);

    /// Decrement the reference count on a port. When the count reaches
    /// zero, libports invokes the class's `clean_routine` and frees the
    /// port.
    pub fn ports_port_deref(port: *mut c_void);
}
