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
    /// Packed bitfields: bits 0..7 msgt_name, 8..23 msgt_size (in bits),
    /// 24..28 msgt_unused (must be 0), 29 msgt_inline, 30 msgt_longform,
    /// 31 msgt_deallocate. Layout matches gnumach <mach/message.h> under
    /// __LP64__.
    pub bits: u32,
    /// msgt_number: how many elements of msgt_size bits each. For a
    /// single scalar this is 1.
    pub number: u32,
}

/// Build a mach_msg_type_t descriptor at compile time. `size_bits` is
/// the per-element bit count (32 for an int, 64 for a port name on LP64).
#[inline]
pub const fn mig_type(name: u8, size_bits: u16, inline: bool) -> mach_msg_type_t {
    let bits = (name as u32)
             | ((size_bits as u32) << 8)
             | ((inline as u32) << 29);
    mach_msg_type_t { bits, number: 1 }
}

/// Canonical descriptor for a single inline 32-bit signed integer.
pub const MIG_TYPE_INT32: mach_msg_type_t =
    mig_type(MACH_MSG_TYPE_INTEGER_32 as u8, 32, true);

/// Size in bits the kernel expects for an inline port descriptor on
/// this target. The check in `ipc_kmsg_copyin_body`
/// (`size != PORT_T_SIZE_IN_BITS`) reads `PORT_T_SIZE_IN_BITS` as
/// `sizeof(mach_port_t)*8`, and *in the kernel* `mach_port_t` is
/// `vm_offset_t` (a pointer-sized integer). So the descriptor must
/// declare 64 bits on x86_64 even though only the low 32 carry the
/// user-side port name.
#[cfg(target_pointer_width = "64")]
pub const PORT_T_SIZE_IN_BITS: u32 = 64;
#[cfg(target_pointer_width = "32")]
pub const PORT_T_SIZE_IN_BITS: u32 = 32;

/// Descriptor for a single inline send-right port name with COPY_SEND
/// disposition (the most common outbound disposition — gives the
/// receiver a copy of our send right while we keep ours). Used in port-
/// transferring RPCs like fsys_startup.
pub const MIG_TYPE_PORT_COPY_SEND: mach_msg_type_t =
    mig_type(MACH_MSG_TYPE_COPY_SEND as u8, PORT_T_SIZE_IN_BITS as u16, true);

/// Descriptor for a single inline send-right port name with MOVE_SEND
/// disposition (transfer ownership). Used in reply paths where the
/// server hands a port-right to the caller — e.g. realnode in
/// fsys_startup's reply.
pub const MIG_TYPE_PORT_MOVE_SEND: mach_msg_type_t =
    mig_type(MACH_MSG_TYPE_MOVE_SEND as u8, PORT_T_SIZE_IN_BITS as u16, true);

/// `msgh_bits` flag indicating the message carries port-right
/// references (so the kernel knows to translate the inline names).
/// Set whenever any descriptor names a PORT_* type — otherwise the
/// kernel treats the message as plain data and the port names won't
/// be translated.
pub const MACH_MSGH_BITS_COMPLEX: mach_msg_bits_t = 0x80000000;

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

// Wire-format byte pattern of MIG_TYPE_INT32 on x86_64 LP64 GNU Mach.
// Canonical reference is gnumach's <mach/message.h> mach_msg_type_t
// definition under #ifdef __LP64__. Expected bytes (little-endian):
//
//   byte 0      : msgt_name  = MACH_MSG_TYPE_INTEGER_32 = 0x02
//   bytes 1..2  : msgt_size  = 32  (16-bit field)        = 0x20 0x00
//   byte 3      : bits 24..28 msgt_unused = 0
//                 bit  29     msgt_inline = 1            -> 0x20
//   bytes 4..7  : msgt_number = 1 (u32 LE)               = 0x01 0x00 0x00 0x00
//
// If this assertion fires, the bitfield encoding in mach_msg_type_t
// does not match the LP64 layout and every MIG send will be rejected
// with MACH_SEND_INVALID_TYPE (0x1000000f).
#[cfg(target_pointer_width = "64")]
const _: () = {
    let bytes = unsafe {
        ::core::mem::transmute::<mach_msg_type_t, [u8; 8]>(MIG_TYPE_INT32)
    };
    assert!(bytes[0] == 0x02);
    assert!(bytes[1] == 0x20);
    assert!(bytes[2] == 0x00);
    assert!(bytes[3] == 0x20);
    assert!(bytes[4] == 0x01);
    assert!(bytes[5] == 0x00);
    assert!(bytes[6] == 0x00);
    assert!(bytes[7] == 0x00);
};

/// Per-port-slot wire shape on LP64 GNU Mach. The kernel union allows
/// stashing a pointer-sized value in the same slot that holds a u32
/// port name. Userspace only ever fills `name`; `kernel_port_do_not_use`
/// exists to document the slot size and is never touched by us.
#[repr(C)]
#[derive(Copy, Clone)]
pub union mach_port_name_inlined_t {
    pub name:                   mach_port_t,
    pub kernel_port_do_not_use: usize,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(::core::mem::size_of::<mach_port_name_inlined_t>() == 8);

/// Descriptor for a port slot whose disposition is supplied by the
/// caller at runtime. The macro fills msgt_name in with the actual
/// disposition (e.g. MACH_MSG_TYPE_COPY_SEND) by overwriting the low
/// 8 bits of the descriptor's `bits` field.
pub const MIG_TYPE_PORT_SEND_POLY: mach_msg_type_t =
    mig_type(MACH_MSG_TYPE_POLYMORPHIC as u8, PORT_T_SIZE_IN_BITS as u16, true);

/// gnumach defines this as `((mach_msg_type_name_t)~0)` — the sentinel
/// telling MIG-generated code "msgt_name is provided at runtime."
pub const MACH_MSG_TYPE_POLYMORPHIC: mach_msg_type_name_t = !0;

/// MIG-defined error codes used by generated reply-validation.
pub const MIG_REPLY_MISMATCH:  kern_return_t = -304;
pub const MIG_SERVER_DIED:     kern_return_t = -305;
pub const MIG_BAD_ID:          kern_return_t = -302;

/// Reply-header msgh_id that means "the send-once right we used to
/// reply on was destroyed by the server". Treated as MIG_SERVER_DIED.
pub const MACH_NOTIFY_SEND_ONCE: mach_msg_id_t = 70; // mach/notify.h

/// Canonical short-error-reply shape: a server that errors before
/// allocating return ports sends back just header + RetCode descriptor
/// + RetCode + pad, with msgh_size == sizeof(mig_reply_header_t).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct mig_reply_header_t {
    pub head:         mach_msg_header_t,
    pub retcode_type: mach_msg_type_t,
    pub retcode:      kern_return_t,
    pub retcode_pad:  u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(::core::mem::size_of::<mig_reply_header_t>() == 48);

/// Equality-compare two descriptors as raw 8-byte values. Safe on LP64
/// little-endian (every Hurd x86_64 target). Used in reply validation
/// to confirm each out-arg's descriptor matches what the macro expects.
#[inline(always)]
pub unsafe fn bad_typecheck(a: *const mach_msg_type_t,
                             b: *const mach_msg_type_t) -> bool {
    ::core::ptr::read(a as *const u64) != ::core::ptr::read(b as *const u64)
}

extern "C" {
    /// libc-provided MIG reply-port cache. Cheaper than `mach_reply_port`
    /// for hot RPC paths because it reuses a thread-local port instead
    /// of allocating + deallocating a receive right per call.
    pub fn mig_get_reply_port() -> mach_port_t;
    pub fn mig_put_reply_port(port: mach_port_t);
    pub fn mig_dealloc_reply_port(port: mach_port_t);
}

/// `mach_port_right_t` values from `<mach/port.h>`. Used by
/// `mach_port_allocate` to ask the kernel for a fresh port of the named
/// kind (typically RIGHT_RECEIVE for "allocate me a brand-new receive
/// right whose port name is unique in this task").
pub type mach_port_right_t = c_int;
pub const MACH_PORT_RIGHT_SEND:      mach_port_right_t = 0;
pub const MACH_PORT_RIGHT_RECEIVE:   mach_port_right_t = 1;
pub const MACH_PORT_RIGHT_SEND_ONCE: mach_port_right_t = 2;

extern "C" {
    /// Allocate a new port right of the requested kind in `task`'s port
    /// space. On success `*name` holds the port name (a u32). Used to
    /// mint the fingerprint receive rights libtrivfs stashes in
    /// `trivfs_control::{filesys_id, file_id}` so clients can compare
    /// "is this the same filesystem / same file" across opens.
    pub fn mach_port_allocate(
        task:  mach_port_t,
        right: mach_port_right_t,
        name:  *mut mach_port_t,
    ) -> kern_return_t;
}

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
