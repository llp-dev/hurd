//! Mach microkernel FFI declarations used by Hurd servers.
//!
//! Hurd's Mach API is exposed through glibc (the same .so that holds
//! POSIX); there is no separate libmach to link against. So this crate
//! is purely declarations — its symbols are resolved against libc.so
//! via the libc-sys crate's #[link(name = "c")].
//!
//! Only the surface that current Rust consumers need is bound; grow as
//! needed.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

pub use libc::{c_int, c_uint, c_void};

// ---- core scalar types ----

/// Kernel return code (success = 0, error otherwise).
pub type kern_return_t = c_int;

/// Mach port name (in this task's port namespace).
pub type mach_port_t = c_uint;

pub type mach_msg_bits_t = c_uint;
pub type mach_msg_size_t = c_uint;
pub type mach_msg_id_t = c_int;
pub type mach_msg_type_name_t = c_uint;
pub type mach_msg_type_number_t = c_uint;
pub type mach_msg_timeout_t = c_uint;
pub type mach_msg_option_t = c_int;
pub type mach_msg_return_t = kern_return_t;

// ---- constants ----

pub const MACH_PORT_NULL: mach_port_t = 0;
pub const MACH_PORT_DEAD: mach_port_t = !0;

pub const KERN_SUCCESS:   kern_return_t = 0;
pub const KERN_FAILURE:   kern_return_t = 5;

// mach_msg() option flags
pub const MACH_MSG_OPTION_NONE: mach_msg_option_t = 0;
pub const MACH_SEND_MSG:        mach_msg_option_t = 0x00000001;
pub const MACH_RCV_MSG:         mach_msg_option_t = 0x00000002;

// msgh_bits encoding helpers — top byte is reserved, then complex/local/remote
pub const MACH_MSG_TYPE_MOVE_RECEIVE:   mach_msg_type_name_t = 16;
pub const MACH_MSG_TYPE_MOVE_SEND:      mach_msg_type_name_t = 17;
pub const MACH_MSG_TYPE_MOVE_SEND_ONCE: mach_msg_type_name_t = 18;
pub const MACH_MSG_TYPE_COPY_SEND:      mach_msg_type_name_t = 19;
pub const MACH_MSG_TYPE_MAKE_SEND:      mach_msg_type_name_t = 20;
pub const MACH_MSG_TYPE_MAKE_SEND_ONCE: mach_msg_type_name_t = 21;

/// Encode msgh_bits: low 5 bits = remote port disposition, bits 8-12 = local.
#[inline]
pub const fn MACH_MSGH_BITS(remote: mach_msg_type_name_t, local: mach_msg_type_name_t) -> mach_msg_bits_t {
    (remote & 0x1f) | ((local & 0x1f) << 8)
}

// ---- message header ----

/// Mach message header — every IPC message starts with this 24-byte block.
/// Layout matches gnumach's `<mach/message.h>`.
///
/// Derives Copy so message structs can be members of a #[repr(C)] union
/// (the standard MIG-style request/reply marshalling pattern).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mach_msg_header_t {
    pub msgh_bits:        mach_msg_bits_t,
    pub msgh_size:        mach_msg_size_t,
    pub msgh_remote_port: mach_port_t,
    pub msgh_local_port:  mach_port_t,
    pub msgh_seqno:       mach_msg_size_t,
    pub msgh_id:          mach_msg_id_t,
}

/// Extract the remote-port disposition bits from msgh_bits. Used when
/// constructing reply messages — the reply's remote-port disposition is
/// the request's remote disposition.
#[inline]
pub const fn MACH_MSGH_BITS_REMOTE(bits: mach_msg_bits_t) -> mach_msg_bits_t {
    bits & 0x1f
}

// ---- NDR (Network Data Representation) record ----
//
// MIG-marshalled messages start with an NDR record after the header.
// The record describes byte order, char encoding, etc. of the wire
// payload. For x86 little-endian ASCII glibc-on-Hurd, the constant
// value `NDR_record` (defined in glibc's ndr.h) is:
//   { mig_reserved: 0, mig_reserved: 0, mig_reserved: 0,
//     int_rep: 0, char_rep: 0, float_rep: 0, mig_reserved: 0 }

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NDR_record_t {
    pub mig_vers:     u8,
    pub if_vers:      u8,
    pub reserved1:    u8,
    pub mig_encoding: u8,
    pub int_rep:      u8,
    pub char_rep:     u8,
    pub float_rep:    u8,
    pub reserved2:    u8,
}

/// The canonical NDR record value for our host (little-endian, ASCII).
/// MIG-generated code emits this exact byte pattern at the start of every
/// marshalled payload. We use it the same way for hand-written stubs.
pub const NDR_RECORD: NDR_record_t = NDR_record_t {
    mig_vers:     0,
    if_vers:      0,
    reserved1:    0,
    mig_encoding: 0,
    int_rep:      0,
    char_rep:     0,
    float_rep:    0,
    reserved2:    0,
};

// ---- extern functions ----
//
// Note: `mach_task_self()` is conventionally a macro in C that reads the
// global `__mach_task_self_`. We bind the global directly and provide a
// thin Rust function for ergonomics.

extern "C" {
    pub static __mach_task_self_: mach_port_t;

    pub fn mach_port_deallocate(task: mach_port_t, name: mach_port_t) -> kern_return_t;

    pub fn task_get_bootstrap_port(task: mach_port_t, port: *mut mach_port_t)
        -> kern_return_t;

    pub fn mach_msg(
        msg:          *mut mach_msg_header_t,
        option:       mach_msg_option_t,
        send_size:    mach_msg_size_t,
        rcv_size:     mach_msg_size_t,
        rcv_name:     mach_port_t,
        timeout:      mach_msg_timeout_t,
        notify:       mach_port_t,
    ) -> mach_msg_return_t;
}

#[inline]
pub fn mach_task_self() -> mach_port_t {
    unsafe { __mach_task_self_ }
}
