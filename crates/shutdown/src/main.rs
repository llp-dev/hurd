// A trivfs-based shutdown translator for the Hurd, ported from shutdown.c.
//
// Copyright (C) 2018 Free Software Foundation, Inc.
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
// Implements one RPC: shutdown_shutdown, which asks the ACPI server to
// put the machine into sleep state S5 (power off).
//
// MIG marshalling is hand-written via mig::routine_call! (outbound,
// for acpi_sleep) and mig::routine_serve! (inbound, for shutdown).
// No MIG tool dependency.

#![no_std]
#![no_main]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use core::ptr::null_mut;

use hurd_rt::{c_char, c_int};

use libc::error;
use mach_sys::{
    kern_return_t, mach_msg_header_t, mach_port_deallocate, mach_port_t,
    mach_task_self, task_get_bootstrap_port, MACH_PORT_NULL,
};
use ports_sys::{
    port_bucket, port_class, ports_count_class, ports_enable_class,
    ports_inhibit_class_rpcs, ports_manage_port_operations_multithread,
    ports_resume_class_rpcs,
};
use trivfs::{
    trivfs_add_control_port_class, trivfs_add_port_bucket,
    trivfs_add_protid_port_class, trivfs_control, trivfs_demuxer, trivfs_protid_t,
    trivfs_startup, FSTYPE_MISC, FSYS_GOAWAY_FORCE, O_READ, O_WRITE,
};

// ---- ACPI sleep states ----
// 3 = S3 (suspend-to-RAM), 5 = S5 (soft power off). We use S5.
const SLEEP_STATE_S5: c_int = 5;

// ---- POSIX errno values used in handlers ----
// Hurd glibc uses standard Linux-numbered errnos here:
const EIO:  kern_return_t = 5;
const EBUSY: kern_return_t = 16;

// ---- File-name lookup (extern from glibc) ----
//
// glibc provides file_name_lookup(name, flags, mode) on Hurd. It walks
// the filesystem (going through trivfs translators) and returns the port
// to the resolved file. Used here to find /servers/acpi.

extern "C" {
    fn file_name_lookup(
        file_name: *const c_char,
        flags:     c_int,
        mode:      c_int,
    ) -> mach_port_t;
}

const O_RDWR: c_int = 2; // Hurd-glibc fcntl flag for read+write

unsafe fn get_acpi() -> mach_port_t {
    // The /servers/acpi path is the conventional location for the ACPI
    // server's port; matches hurd/paths.h's _SERVERS_ACPI.
    let path = b"/servers/acpi\0".as_ptr() as *const c_char;
    file_name_lookup(path, O_RDWR, 0)
}

// ---- Outbound MIG: acpi_sleep ----
//
// Generates: unsafe fn acpi_sleep(server: mach_port_t, sleep_state: c_int)
//            -> kern_return_t
//
// Subsystem `acpi` is 41000 in hurd/acpi.defs; acpi_sleep is the first
// routine, so msgh_id = 41000.
mig::routine_call! {
    fn acpi_sleep(sleep_state: c_int) = 41000;
}

// ---- Inbound MIG: shutdown_shutdown ----
//
// The user-provided handler. Called by the macro-generated dispatcher
// with the request's server port as the first arg.

unsafe fn handle_shutdown_shutdown(_server: mach_port_t) -> kern_return_t {
    let acpi = get_acpi();
    if acpi == MACH_PORT_NULL {
        return EIO;
    }
    let err = acpi_sleep(acpi, SLEEP_STATE_S5);
    mach_port_deallocate(mach_task_self(), acpi);
    err
}

// Generates: unsafe extern "C" fn shutdown_server_routine(inp, outp) -> c_int
//
// Subsystem `shutdown` is 40000 in hurd/shutdown.defs; the single routine
// shutdown_shutdown sits at offset 0, so msgh_id = 40000.
mig::routine_serve! {
    fn shutdown_server_routine for msgh_id 40000;
    handler: handle_shutdown_shutdown() -> kern_return_t;
}

// ---- Trivfs globals ----
//
// libtrivfs declares these as extern; we provide the strong definitions.
// #[no_mangle] is required so the linker's symbol-resolution sees them
// under the bare C names libtrivfs references.

#[no_mangle]
pub static mut trivfs_fstype:        c_int = FSTYPE_MISC;
#[no_mangle]
pub static mut trivfs_fsid:          c_int = 0;
#[no_mangle]
pub static mut trivfs_support_read:  c_int = 0;
#[no_mangle]
pub static mut trivfs_support_write: c_int = 0;
#[no_mangle]
pub static mut trivfs_support_exec:  c_int = 0;
#[no_mangle]
pub static mut trivfs_allow_open:    c_int = O_READ | O_WRITE;

// ---- Trivfs callbacks ----
//
// trivfs_modify_stat is called by libtrivfs to let us adjust the
// returned stat info before sending. For /servers/shutdown the default
// stat is fine, so this is a no-op.

#[no_mangle]
pub unsafe extern "C" fn trivfs_modify_stat(
    _cred: trivfs_protid_t,
    _st:   *mut core::ffi::c_void,
) {
}

// Globals holding our port bucket / classes — needed by trivfs_goaway.
static mut PORT_BUCKET:          *mut port_bucket = null_mut();
static mut TRIVFS_CONTROL_CLASS: *mut port_class  = null_mut();
static mut TRIVFS_PROTID_CLASS:  *mut port_class  = null_mut();

#[no_mangle]
pub unsafe extern "C" fn trivfs_goaway(
    _fsys: *mut trivfs_control,
    flags: c_int,
) -> kern_return_t {
    // Stop new requests.
    ports_inhibit_class_rpcs(TRIVFS_CONTROL_CLASS);
    ports_inhibit_class_rpcs(TRIVFS_PROTID_CLASS);

    // If users hold open file handles to /servers/shutdown and we're
    // not being forced, refuse to go away.
    let count = ports_count_class(TRIVFS_PROTID_CLASS);
    if count > 0 && (flags & FSYS_GOAWAY_FORCE) == 0 {
        ports_enable_class(TRIVFS_PROTID_CLASS);
        ports_resume_class_rpcs(TRIVFS_CONTROL_CLASS);
        ports_resume_class_rpcs(TRIVFS_PROTID_CLASS);
        return EBUSY;
    }

    libc::exit(0);
}

// ---- Combined demuxer ----
//
// Mirrors shutdown.c's shutdown_demuxer: tries the shutdown-specific
// routine first (one RPC, msgh_id 40000), then falls back to libtrivfs's
// own demuxer for the generic fs.defs / io.defs RPCs.

unsafe extern "C" fn combined_demuxer(
    inp:  *mut mach_msg_header_t,
    outp: *mut mach_msg_header_t,
) -> c_int {
    if shutdown_server_routine(inp, outp) != 0 {
        return 1;
    }
    trivfs_demuxer(inp, outp)
}

// ---- Entry point ----

// Tiny stderr-only diagnostic. Using write(2, ...) directly avoids any
// printf-style formatting machinery so we can see traces even when libc
// or other init is in a bad state.
unsafe fn dbg(msg: &[u8]) {
    let _ = libc::write(2, msg.as_ptr() as *const core::ffi::c_void, msg.len());
}

#[hurd_rt::entry]
fn main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    dbg(b"shutdown: entered main\n");

    let mut bootstrap: mach_port_t = 0;
    dbg(b"shutdown: calling task_get_bootstrap_port\n");
    let kr = task_get_bootstrap_port(mach_task_self(), &mut bootstrap);
    dbg(b"shutdown: task_get_bootstrap_port returned, kr=0x");
    // Print kr as 8 hex digits to stderr.
    let mut hex = [b'0'; 8];
    let bytes = (kr as u32).to_be_bytes();
    let hexchars = b"0123456789abcdef";
    for i in 0..4 {
        hex[i * 2]     = hexchars[(bytes[i] >> 4) as usize];
        hex[i * 2 + 1] = hexchars[(bytes[i] & 0x0f) as usize];
    }
    let _ = libc::write(2, hex.as_ptr() as *const core::ffi::c_void, hex.len());
    dbg(b"\n");

    if bootstrap == MACH_PORT_NULL {
        dbg(b"shutdown: bootstrap is NULL\n");
        error(
            1, 0,
            b"must be started as a translator\0".as_ptr() as *const c_char,
        );
    }
    dbg(b"shutdown: bootstrap non-NULL\n");

    // &raw mut instead of &mut on these statics: Rust 2024 edition lints
    // against creating actual mutable references to mutable statics because
    // any other reference could exist concurrently. Raw pointers don't
    // have that aliasing requirement and are what libtrivfs actually
    // wants anyway.
    if trivfs_add_port_bucket(&raw mut PORT_BUCKET) != 0 {
        error(
            1, 0,
            b"error creating port bucket\0".as_ptr() as *const c_char,
        );
    }
    if trivfs_add_control_port_class(&raw mut TRIVFS_CONTROL_CLASS) != 0 {
        error(
            1, 0,
            b"error creating control port class\0".as_ptr() as *const c_char,
        );
    }
    if trivfs_add_protid_port_class(&raw mut TRIVFS_PROTID_CLASS) != 0 {
        error(
            1, 0,
            b"error creating protid port class\0".as_ptr() as *const c_char,
        );
    }

    dbg(b"shutdown: calling trivfs_startup\n");
    let mut fsys: *mut trivfs_control = null_mut();
    let err = trivfs_startup(
        bootstrap, 0,
        TRIVFS_CONTROL_CLASS, PORT_BUCKET,
        TRIVFS_PROTID_CLASS,  PORT_BUCKET,
        &mut fsys,
    );
    dbg(b"shutdown: trivfs_startup returned\n");
    mach_port_deallocate(mach_task_self(), bootstrap);
    if err != 0 {
        error(
            3, err,
            b"Contacting parent\0".as_ptr() as *const c_char,
        );
    }
    dbg(b"shutdown: entering main loop\n");

    // Service requests forever. ports_manage_port_operations_multithread
    // can return (it has timeouts on idle worker threads); restart it.
    loop {
        ports_manage_port_operations_multithread(
            PORT_BUCKET,
            combined_demuxer,
            2 * 60 * 1000,
            10 * 60 * 1000,
            None,
        );
    }
}
