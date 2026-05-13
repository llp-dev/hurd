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

// ---- complex-message descriptor types ----
//
// "Complex" Mach messages carry typed descriptors after the header
// (ports, out-of-line memory, etc.). The msgh_bits COMPLEX flag tells
// the kernel/server to interpret what follows the header as a
// descriptor table rather than raw NDR-encoded scalars.

pub const MACH_MSGH_BITS_COMPLEX: mach_msg_bits_t = 0x80000000;

/// Body header: how many descriptors follow.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mach_msg_body_t {
    pub msgh_descriptor_count: mach_msg_size_t,
}

/// Descriptor for transferring a single port right. Used as the reply
/// payload for routines like task_get_special_port. On GNU Mach the
/// `bits` u32 packs pad2:16 | disposition:8 | type:8 from low to high.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mach_msg_port_descriptor_t {
    pub name: mach_port_t,
    pub pad1: mach_msg_size_t,
    pub bits: u32,
}

/// Descriptor-type tag for a single port-right descriptor (vs out-of-line
/// memory, port arrays, etc.).
pub const MACH_MSG_PORT_DESCRIPTOR: u32 = 0;

/// MIG-defined error code for a mismatched message type or descriptor
/// shape. Returned by our hand-written stubs when the reply doesn't
/// look how we expect.
pub const MIG_TYPE_ERROR: kern_return_t = -303;

// ---- task special-port indices ----
//
// task_get_special_port takes a "which_port" int identifying which of
// the task's well-known ports the caller wants. Bootstrap port (the
// translator's parent / control port) is 4 on GNU Mach.

pub const TASK_KERNEL_PORT:    c_int = 1;
pub const TASK_HOST_PORT:      c_int = 2;
pub const TASK_NAME_PORT:      c_int = 3;
pub const TASK_BOOTSTRAP_PORT: c_int = 4;

/// MIG msgh_id for task_get_special_port. Subsystem `task` is 3400 on
/// GNU Mach and this is routine offset 10 → 3410. Verified against
/// gnumach's mach/task.defs.
const TASK_GET_SPECIAL_PORT_ID: mach_msg_id_t = 3410;

// ---- extern functions ----
//
// Notes:
//   - `mach_task_self()` is conventionally a macro in C that reads the
//     global `__mach_task_self_`. We bind the global directly and
//     provide a thin Rust function below.
//   - `task_get_bootstrap_port` is *not* a real symbol in any Hurd
//     library — it's a `static inline` in <mach/task.h> that wraps
//     `task_get_special_port`. We provide our own Rust implementation
//     so we don't depend on the header.

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
}

#[inline]
pub fn mach_task_self() -> mach_port_t {
    unsafe { __mach_task_self_ }
}

/// Hand-written client stub for the Mach RPC
/// `task_get_special_port(target, which_port, out_port)`.
///
/// Wire format (request):  Head + NDR + which_port:int           = 36 bytes
/// Wire format (reply):    Head + body + port_descriptor          = 36 bytes
///
/// The reply's msgh_bits has MACH_MSGH_BITS_COMPLEX set; the kernel
/// transfers the requested port right (TASK_*_PORT) to us via the
/// `mach_msg_port_descriptor_t` payload.
pub unsafe fn task_get_special_port(
    target_task: mach_port_t,
    which_port:  c_int,
    out:         *mut mach_port_t,
) -> kern_return_t {
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Req {
        head:       mach_msg_header_t,
        ndr:        NDR_record_t,
        which_port: c_int,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Rep {
        head: mach_msg_header_t,
        body: mach_msg_body_t,
        port: mach_msg_port_descriptor_t,
    }
    #[repr(C)]
    union Buf { req: Req, rep: Rep }

    let mut buf = Buf {
        req: Req {
            head: mach_msg_header_t {
                msgh_bits:        0,
                msgh_size:        0,
                msgh_remote_port: 0,
                msgh_local_port:  0,
                msgh_seqno:       0,
                msgh_id:          0,
            },
            ndr:        NDR_RECORD,
            which_port,
        },
    };

    let reply_port = mach_reply_port();

    buf.req.head.msgh_bits =
        MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE);
    buf.req.head.msgh_size        = core::mem::size_of::<Req>() as u32;
    buf.req.head.msgh_remote_port = target_task;
    buf.req.head.msgh_local_port  = reply_port;
    buf.req.head.msgh_id          = TASK_GET_SPECIAL_PORT_ID;

    // 5-second timeout on RECV so we never hang forever during dev.
    // If this fires (MACH_RCV_TIMED_OUT) it means our SEND completed
    // but the kernel didn't reply within 5s — likely the kernel
    // silently dropped our message because its parse failed.
    let ret = mach_msg(
        &mut buf.req.head as *mut _,
        MACH_SEND_MSG | MACH_RCV_MSG | MACH_RCV_TIMEOUT,
        core::mem::size_of::<Req>() as u32,
        core::mem::size_of::<Buf>() as u32,
        reply_port,
        5000,
        MACH_PORT_NULL,
    );
    if ret != KERN_SUCCESS {
        return ret;
    }

    // The kernel must have sent exactly one descriptor, of port type,
    // disposition SEND. If not, the reply is malformed.
    if buf.rep.body.msgh_descriptor_count != 1 {
        return MIG_TYPE_ERROR;
    }
    let disposition = (buf.rep.port.bits >> 16) & 0xff;
    if disposition != MACH_MSG_TYPE_MOVE_SEND
        && disposition != MACH_MSG_TYPE_COPY_SEND
    {
        return MIG_TYPE_ERROR;
    }

    *out = buf.rep.port.name;
    KERN_SUCCESS
}

/// Convenience wrapper: fetch the task's bootstrap port (its translator
/// parent / control port). The C header `<mach/task.h>` defines this
/// as a static inline wrapping `task_get_special_port` with
/// `TASK_BOOTSTRAP_PORT`.
#[inline]
pub unsafe fn task_get_bootstrap_port(
    target_task: mach_port_t,
    out:         *mut mach_port_t,
) -> kern_return_t {
    task_get_special_port(target_task, TASK_BOOTSTRAP_PORT, out)
}
