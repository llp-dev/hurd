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
pub const MACH_SEND_TIMEOUT:    mach_msg_option_t = 0x00000010;
pub const MACH_RCV_TIMEOUT:     mach_msg_option_t = 0x00000100;

// mach_msg return codes commonly used for diagnostics
pub const MACH_SEND_INVALID_DEST: mach_msg_return_t = 0x10000003;
pub const MACH_RCV_TIMED_OUT:     mach_msg_return_t = 0x10004003;
pub const MACH_RCV_TOO_LARGE:     mach_msg_return_t = 0x10004004;

// msgh_bits encoding helpers — top byte is reserved, then complex/local/remote
// Element-type tags for inline scalar arguments (msgt_name field of
// mach_msg_type_t). See gnumach's <mach/message.h>.
pub const MACH_MSG_TYPE_UNSTRUCTURED:   mach_msg_type_name_t =  0;
pub const MACH_MSG_TYPE_INTEGER_16:     mach_msg_type_name_t =  1;
pub const MACH_MSG_TYPE_INTEGER_32:     mach_msg_type_name_t =  2;
pub const MACH_MSG_TYPE_CHAR:           mach_msg_type_name_t =  8;
pub const MACH_MSG_TYPE_BYTE:           mach_msg_type_name_t =  9;
pub const MACH_MSG_TYPE_REAL:           mach_msg_type_name_t = 10;
pub const MACH_MSG_TYPE_INTEGER_64:     mach_msg_type_name_t = 11;
pub const MACH_MSG_TYPE_STRING:         mach_msg_type_name_t = 12;

// Port-right transfer dispositions (used in msgh_bits, not as msgt_name).
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

/// Mach message header — every IPC message starts with this 32-byte block
/// on x86_64-gnu.
///
/// Layout matches gnumach's `<mach/message.h>`. The port-name fields live
/// inside an anonymous union with `rpc_uintptr_t` (so the kernel can stash
/// a pointer in the same slot without rewriting the message on 64-bit):
///
/// ```c
/// typedef struct mach_msg_header {
///     mach_msg_bits_t  msgh_bits;       // 4
///     mach_msg_size_t  msgh_size;       // 4
///     union { mach_port_t msgh_remote_port; rpc_uintptr_t pad; };  // 8
///     union { mach_port_t msgh_local_port;  rpc_uintptr_t pay; };  // 8
///     mach_port_seqno_t msgh_seqno;     // 4
///     mach_msg_id_t     msgh_id;        // 4
/// } mach_msg_header_t;                  // total: 32
/// ```
///
/// We model each port union with an explicit `u32` port-name field
/// followed by 4 bytes of pad, matching the byte layout. Userspace only
/// ever reads/writes the low 32 bits; the high 32 are zero on send and
/// ignored on receive.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mach_msg_header_t {
    pub msgh_bits:         mach_msg_bits_t,
    pub msgh_size:         mach_msg_size_t,
    pub msgh_remote_port:  mach_port_t,
    pub _msgh_remote_pad:  u32,
    pub msgh_local_port:   mach_port_t,
    pub _msgh_local_pad:   u32,
    pub msgh_seqno:        mach_msg_size_t,
    pub msgh_id:           mach_msg_id_t,
}

/// Extract the remote-port disposition bits from msgh_bits. Used when
/// constructing reply messages — the reply's remote-port disposition is
/// the request's remote disposition.
#[inline]
pub const fn MACH_MSGH_BITS_REMOTE(bits: mach_msg_bits_t) -> mach_msg_bits_t {
    bits & 0x1f
}

// ---- mach_msg_type_t — old-format MIG inline-argument descriptor ----
//
// Modern (Darwin/NeXTSTEP) Mach uses NDR records + per-arg descriptors
// only for complex messages. GNU Mach kept the older Mach 2.5 protocol:
// every inline scalar argument is preceded by an 8-byte (on x86_64)
// `mach_msg_type_t` that names its element type, size in *bits*, count,
// and inline-vs-out-of-line flag.
//
// In C this is a packed 32-bit bitfield with `aligned(uintptr_t)`:
//   struct {
//     unsigned int msgt_name : 8,
//                  msgt_size : 8,        /* size in bits! */
//                  msgt_number : 12,
//                  msgt_inline : 1,
//                  msgt_longform : 1,
//                  msgt_deallocate : 1,
//                  msgt_unused : 1;
//   } __attribute__ ((aligned (__alignof__ (uintptr_t))));
//
// We model the 32 packed bits as a single u32 and force 8-byte
// alignment with repr(C, align(8)) so embedded use matches C layout.
// Encoding is LSB-first (matches GCC bitfields on little-endian x86).

#[repr(C, align(8))]
#[derive(Copy, Clone)]
pub struct mach_msg_type_t {
    pub bits: u32,
}

/// Canonical descriptor for a single inline 32-bit signed integer
/// (msgt_name = INTEGER_32, msgt_size = 32, msgt_number = 1, inline = 1).
pub const MIG_TYPE_INT32: mach_msg_type_t = mach_msg_type_t {
    bits: (MACH_MSG_TYPE_INTEGER_32 as u32)       // bits  0..7  msgt_name
        | (32u32 << 8)                            // bits  8..15 msgt_size (BITS)
        | (1u32  << 16)                           // bits 16..27 msgt_number
        | (1u32  << 28),                          // bit  28     msgt_inline
};

/// MIG-defined error code for a mismatched message type or descriptor
/// shape. Returned by handlers when a reply doesn't look how we expect.
pub const MIG_TYPE_ERROR: kern_return_t = -303;

/// Trait for scalar types that can be marshalled as a MIG inline argument.
/// The `mig` crate's `routine_call!` / `routine_serve!` macros consult
/// `<$ty as MigScalar>::TYPE` to emit the right descriptor for each
/// argument. Impl this for any new scalar you need to send over MIG.
pub trait MigScalar: Copy {
    const TYPE: mach_msg_type_t;
}

impl MigScalar for c_int {
    const TYPE: mach_msg_type_t = MIG_TYPE_INT32;
}

/// Tagged inline argument: an 8-byte (on x86_64) `mach_msg_type_t`
/// descriptor followed by the value itself.
///
/// `repr(C, align(8))` ensures the next slot in a message struct lands
/// on the 8-byte boundary the C-side stubs expect — see
/// `mach.user.c`'s generated `Request` layout in the gnumach build:
/// the int that follows the descriptor is padded out to a multiple of
/// the word size so subsequent descriptors stay aligned.
#[repr(C, align(8))]
#[derive(Copy, Clone)]
pub struct Tagged<T: Copy + MigScalar> {
    pub descriptor: mach_msg_type_t,
    pub value:      T,
}

// ---- layout assertions (x86_64 GNU Mach userspace) ----
//
// These are the wire-format invariants we depend on. If any of these
// fire at compile time, message marshalling will be off by exactly the
// difference and the kernel will reject our sends with
// MACH_SEND_MSG_TOO_SMALL or similar.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(::core::mem::size_of::<mach_msg_header_t>() == 32);
    assert!(::core::mem::size_of::<mach_msg_type_t>()   ==  8);
    assert!(::core::mem::size_of::<Tagged<c_int>>()     == 16);
};

// ---- task special-port indices ----
//
// task_get_special_port takes a "which_port" int identifying which of
// the task's well-known ports the caller wants. Bootstrap port (the
// translator's parent / control port) is 4 on GNU Mach.

pub const TASK_KERNEL_PORT:    c_int = 1;
pub const TASK_HOST_PORT:      c_int = 2;
pub const TASK_NAME_PORT:      c_int = 3;
pub const TASK_BOOTSTRAP_PORT: c_int = 4;

// ---- extern functions ----
//
// `task_get_special_port` IS a real symbol in libc.so.0.3 on Hurd
// (verified via `nm -D /usr/lib/.../libc.so.0.3`). What `<mach/task.h>`
// publishes as a static inline is the `task_get_bootstrap_port`
// convenience macro, which trivially wraps the real RPC stub. So:
//
//   - We declare `task_get_special_port` as `extern "C"` and let the
//     linker resolve it (via -lc; libmachuser is a transitional alias
//     in the modern Hurd port).
//   - We provide a Rust `task_get_bootstrap_port` inline that does the
//     same wrap the C header does, no marshalling required.
//
// `mach_task_self()` is a macro in C that reads the global
// `__mach_task_self_`. We bind the global directly and provide a thin
// Rust function below.

extern "C" {
    pub static __mach_task_self_: mach_port_t;

    pub fn mach_port_deallocate(task: mach_port_t, name: mach_port_t) -> kern_return_t;

    pub fn mach_msg(
        msg:          *mut mach_msg_header_t,
        option:       mach_msg_option_t,
        send_size:    mach_msg_size_t,
        rcv_size:     mach_msg_size_t,
        rcv_name:     mach_port_t,
        timeout:      mach_msg_timeout_t,
        notify:       mach_port_t,
    ) -> mach_msg_return_t;

    /// Allocate a fresh receive right in the current task; returns its
    /// port name. Used as the reply port for outbound RPCs.
    pub fn mach_reply_port() -> mach_port_t;

    /// Retrieve one of the task's well-known special ports
    /// (kernel/host/name/bootstrap). MIG-generated stub from
    /// `mach/mach.defs`.
    pub fn task_get_special_port(
        target_task: mach_port_t,
        which_port:  c_int,
        out:         *mut mach_port_t,
    ) -> kern_return_t;
}

#[inline]
pub fn mach_task_self() -> mach_port_t {
    unsafe { __mach_task_self_ }
}

/// Fetch the task's bootstrap port (its translator parent / control port).
/// `<mach/task.h>` defines this as a static inline wrapping
/// `task_get_special_port(TASK_BOOTSTRAP_PORT)` — we mirror that exactly.
#[inline]
pub unsafe fn task_get_bootstrap_port(
    target_task: mach_port_t,
    out:         *mut mach_port_t,
) -> kern_return_t {
    task_get_special_port(target_task, TASK_BOOTSTRAP_PORT, out)
}
