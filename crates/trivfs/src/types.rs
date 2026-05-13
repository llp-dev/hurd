//! C-ABI-compatible layouts for `struct port_info`, `struct trivfs_control`,
//! `struct trivfs_protid`, and `struct trivfs_peropen`.
//!
//! These types are deliberately **layout-bound** to the C definitions in
//! `<hurd/ports.h>` and `<hurd/trivfs.h>`. libports allocates them via
//! `ports_create_port(class, bucket, size, &result)` where `size` is the
//! full size of our outer struct — libports never reads our private
//! tail, it only manipulates the `port_info` header at offset 0.
//!
//! Equally, when libports invokes our `clean_routine` callback with
//! `void *port`, that pointer is `&port_info`. Because `port_info` is
//! the first field of `trivfs_control`/`trivfs_protid`, the same
//! pointer (cast to `*mut Trivfs{Control,Protid}`) gives us the outer
//! struct.
//!
//! Sizes (x86_64-gnu, asserted at the bottom of this file):
//!   - port_info:        64 bytes
//!   - TrivfsControl:    64 + 8 + 8 + 4 + 4 + 4 + 4 + 8 = 104 (with align padding)
//!   - TrivfsProtid:     64 + ... matches `struct trivfs_protid`
//!   - TrivfsPeropen:    matches `struct trivfs_peropen`

use core::ffi::c_void;
use libc::c_int;
use mach_sys::mach_port_t;
use ports_sys::{port_bucket, port_class};

/// Layout-bound mirror of `struct port_info` from `<hurd/ports.h>`.
///
/// Field-by-field layout (x86_64 GNU Hurd):
/// ```text
/// offset  size  field
/// 0       8     class:               *mut port_class
/// 8       8     refcounts:           refcounts_t   (union { u32 value; struct { u32 hard; u32 weak; } })
/// 16      4     mscount:             mach_port_mscount_t  (uint32)
/// 20      4     cancel_threshold:    mach_msg_seqno_t     (uint32, atomic)
/// 24      4     flags:               int
/// 28      4     port_right:          mach_port_t          (uint32 port name)
/// 32      8     current_rpcs:        *mut rpc_info
/// 40      8     bucket:              *mut port_bucket
/// 48      8     hentry:              hurd_ihash_locp_t    (void **)
/// 56      8     ports_htable_entry:  hurd_ihash_locp_t    (void **)
/// total:  64
/// ```
///
/// We treat all pointer fields as raw pointers; libports manages them.
/// `refcounts` is exposed as a `u64` since we never touch it directly —
/// all reference-count manipulation goes through `ports_port_ref` /
/// `ports_port_deref`.
#[repr(C)]
pub struct port_info {
    pub class:               *mut port_class,
    pub refcounts:           u64,
    pub mscount:             u32,
    pub cancel_threshold:    u32,
    pub flags:               c_int,
    pub port_right:          mach_port_t,
    pub current_rpcs:        *mut c_void,
    pub bucket:              *mut port_bucket,
    pub hentry:              *mut *mut c_void,
    pub ports_htable_entry:  *mut *mut c_void,
}

/// `struct trivfs_peropen` — one per open file descriptor against the
/// translator. Multiple protids can share a peropen (after `io_duplicate`).
#[repr(C)]
pub struct TrivfsPeropen {
    pub hook:        *mut c_void,
    pub openmodes:   c_int,
    pub refcnt:      u32,            // refcount_t — single u32, atomic
    pub cntl:        *mut TrivfsControl,
    // We omit lock_status and tp until we need them (rlock state is
    // ~32 bytes of pthread mutex + lists). For shutdown they're unused.
    pub _padding:    [u8; 64],
}

/// `struct trivfs_protid` — one per "active reference" to the translator
/// (i.e. one per file descriptor + duplicates). Embeds port_info at
/// offset 0 so libports's `void *port` callbacks cast cleanly.
#[repr(C)]
pub struct TrivfsProtid {
    pub pi:        port_info,
    pub user:      *mut c_void,           // struct iouser *
    pub isroot:    c_int,
    pub realnode:  mach_port_t,
    pub hook:      *mut c_void,
    pub po:        *mut TrivfsPeropen,
}

/// `struct trivfs_control` — the translator's control port (one per
/// running translator instance). Embeds port_info at offset 0.
#[repr(C)]
pub struct TrivfsControl {
    pub pi:             port_info,
    pub protid_class:   *mut port_class,
    pub protid_bucket:  *mut port_bucket,
    pub filesys_id:     mach_port_t,
    pub file_id:        mach_port_t,
    pub underlying:     mach_port_t,
    pub _pad:           u32,              // align hook to 8
    pub hook:           *mut c_void,
}

// ---- size assertions (x86_64) ----
//
// If C libtrivfs is ever linked alongside our crate (during the
// transition), or if libports's `ports_create_port` is called with our
// size, getting these wrong corrupts the allocator and trashes nearby
// memory. Pin them so the build refuses to produce a binary with a
// drifted layout.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(::core::mem::size_of::<port_info>()    == 64);
    assert!(::core::mem::size_of::<TrivfsControl>() == 104);
    // TrivfsProtid: 64 + 8 + 4 + 4 + 8 + 8 = 96
    assert!(::core::mem::size_of::<TrivfsProtid>()  == 96);
};
