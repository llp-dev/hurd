//! The `trivfs_startup` bootstrap rendezvous.
//!
//! When a translator is exec'd by `settrans`, the parent fs hands it a
//! single mach port (the "bootstrap port") set by
//! `task_set_special_port(TASK_BOOTSTRAP_PORT, ...)` in the launcher.
//! The translator's job is to:
//!
//!   1. Create a control port that represents the running translator
//!      instance (allocated through libports → port class + bucket).
//!   2. Send a `fsys_startup` RPC on the bootstrap port telling the
//!      parent "here is my control-port send right; please give me back
//!      a port to the node I'm translating."
//!   3. Stash the returned `underlying` (a.k.a. realnode) port for use
//!      in later operations like `io_stat`.
//!
//! `fsys_startup`'s wire shape transfers port rights, so we can't use
//! the simple-scalar `mig::routine_call!` macro — we hand-roll the
//! request/reply structs here.

use core::ffi::c_void;
use core::ptr::null_mut;

use libc::{c_int, error_t};
use mach_sys::{
    kern_return_t, mach_msg, mach_msg_header_t, mach_msg_size_t, mach_msg_type_t,
    mach_port_deallocate, mach_port_t, mach_task_self, mach_reply_port,
    KERN_SUCCESS, MACH_MSGH_BITS, MACH_MSGH_BITS_COMPLEX, MACH_MSG_TYPE_COPY_SEND,
    MACH_MSG_TYPE_MAKE_SEND_ONCE, MACH_PORT_NULL, MACH_RCV_MSG, MACH_SEND_MSG,
    MIG_TYPE_INT32, MIG_TYPE_PORT_COPY_SEND,
};
use ports_sys::{
    port_bucket, port_class, ports_create_bucket, ports_create_class,
    ports_create_port, ports_get_send_right, ports_port_deref,
};

use crate::cleanup::{trivfs_clean_cntl, trivfs_clean_protid};
use crate::types::TrivfsControl;

/// msgh_id for `fsys_startup` — subsystem `fsys` is 22000, routine
/// offset 0. Verified against hurd/fsys.defs.
const FSYS_STARTUP_ID: i32 = 22000;

/// Allocate (or reuse) a port bucket. Pass `&mut null_mut()` to get a
/// fresh one allocated for you; pass a pre-allocated bucket pointer to
/// reuse it (we just leave it alone in that case).
///
/// Mirrors libtrivfs's `trivfs_add_port_bucket`. The "add" naming is
/// historical — libtrivfs used to track a list of dynamically-allocated
/// buckets for cleanup. We don't, because shutdown never frees its
/// bucket.
#[no_mangle]
pub unsafe extern "C" fn trivfs_add_port_bucket(
    bucket: *mut *mut port_bucket,
) -> error_t {
    if (*bucket).is_null() {
        let new_bucket = ports_create_bucket();
        if new_bucket.is_null() {
            return ENOMEM;
        }
        *bucket = new_bucket;
    }
    0
}

const ENOMEM: error_t = 12;

/// Allocate (or reuse) a control port class.
#[no_mangle]
pub unsafe extern "C" fn trivfs_add_control_port_class(
    class: *mut *mut port_class,
) -> error_t {
    if (*class).is_null() {
        let new_class = ports_create_class(Some(trivfs_clean_cntl), None);
        if new_class.is_null() {
            return ENOMEM;
        }
        *class = new_class;
    }
    0
}

/// Allocate (or reuse) a protid port class.
#[no_mangle]
pub unsafe extern "C" fn trivfs_add_protid_port_class(
    class: *mut *mut port_class,
) -> error_t {
    if (*class).is_null() {
        let new_class = ports_create_class(Some(trivfs_clean_protid), None);
        if new_class.is_null() {
            return ENOMEM;
        }
        *class = new_class;
    }
    0
}

/// Allocate a trivfs_control with the given underlying node port and
/// register it as a port of `control_class`. Mirrors libtrivfs's
/// `trivfs_create_control`. On success, `*control` is non-NULL and
/// holds a reference (released by the caller via `ports_port_deref`).
unsafe fn create_control(
    underlying:     mach_port_t,
    control_class:  *mut port_class,
    control_bucket: *mut port_bucket,
    protid_class:   *mut port_class,
    protid_bucket:  *mut port_bucket,
    control:        *mut *mut TrivfsControl,
) -> error_t {
    let mut raw: *mut c_void = null_mut();
    let err = ports_create_port(
        control_class,
        control_bucket,
        core::mem::size_of::<TrivfsControl>(),
        &mut raw,
    );
    if err != 0 {
        return err;
    }

    let fsys = raw as *mut TrivfsControl;
    // libports zeroes the allocation, but be explicit about the
    // trivfs-private tail we layer on top.
    (*fsys).protid_class  = protid_class;
    (*fsys).protid_bucket = protid_bucket;
    (*fsys).filesys_id    = 0;
    (*fsys).file_id       = 0;
    (*fsys).underlying    = underlying;
    (*fsys).hook          = null_mut();
    *control = fsys;
    0
}

/// Hand-rolled outbound MIG stub for fsys_startup.
///
/// Wire format (request, 64 bytes):
///   - mach_msg_header_t      (32, COMPLEX | COPY_SEND | MAKE_SEND_ONCE)
///   - Tagged<int>            (16, INTEGER_32 descriptor + openflags)
///   - Tagged<port-send-name> (16, COPY_SEND descriptor + control_port name)
///
/// Wire format (reply on success, 64 bytes, COMPLEX flag set):
///   - mach_msg_header_t      (32)
///   - Tagged<int>            (16, retcode)
///   - Tagged<port-send-name> (16, MOVE_SEND descriptor + realnode name)
///
/// On error the reply is just header + Tagged<retcode> = 48 bytes.
unsafe fn fsys_startup(
    bootstrap:    mach_port_t,
    openflags:    c_int,
    control_port: mach_port_t,
    realnode:     *mut mach_port_t,
) -> kern_return_t {
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct PortSlot {
        descriptor: mach_msg_type_t,
        port_name:  mach_port_t,
        _pad:       u32,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct IntSlot {
        descriptor: mach_msg_type_t,
        value:      c_int,
        _pad:       u32,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Req {
        head:      mach_msg_header_t,
        openflags: IntSlot,
        ctlport:   PortSlot,
    }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct Rep {
        head:     mach_msg_header_t,
        retcode:  IntSlot,
        realnode: PortSlot,
    }
    #[repr(C)]
    union Buf { req: Req, rep: Rep }

    // Sanity: both 64 bytes.
    const _: () = assert!(::core::mem::size_of::<Req>() == 64);
    const _: () = assert!(::core::mem::size_of::<Rep>() == 64);

    let reply_port = mach_reply_port();

    let mut buf = Buf {
        req: Req {
            head: mach_msg_header_t {
                msgh_bits: MACH_MSGH_BITS_COMPLEX
                    | MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE),
                msgh_size:         ::core::mem::size_of::<Req>() as mach_msg_size_t,
                msgh_remote_port:  bootstrap,
                _msgh_remote_pad:  0,
                msgh_local_port:   reply_port,
                _msgh_local_pad:   0,
                msgh_seqno:        0,
                msgh_id:           FSYS_STARTUP_ID,
            },
            openflags: IntSlot {
                descriptor: MIG_TYPE_INT32,
                value:      openflags,
                _pad:       0,
            },
            ctlport: PortSlot {
                descriptor: MIG_TYPE_PORT_COPY_SEND,
                port_name:  control_port,
                _pad:       0,
            },
        },
    };

    // Diagnostic: dump the 64 request bytes we're about to send so we can
    // compare to the mig-generated stub's wire format.
    {
        let bytes = core::slice::from_raw_parts(
            &buf.req as *const _ as *const u8,
            ::core::mem::size_of::<Req>(),
        );
        libc::write(2, b"fsys_startup req:\n\0".as_ptr() as *const _, 18);
        let hexchars = b"0123456789abcdef";
        let mut line = [0u8; 56];
        for row in 0..(bytes.len() / 16) {
            for col in 0..16 {
                let b = bytes[row * 16 + col];
                line[col * 3]     = hexchars[(b >> 4) as usize];
                line[col * 3 + 1] = hexchars[(b & 0x0f) as usize];
                line[col * 3 + 2] = b' ';
            }
            line[48] = b'\n';
            libc::write(2, line.as_ptr() as *const _, 49);
        }
    }

    let ret = mach_msg(
        &mut buf.req.head as *mut _,
        MACH_SEND_MSG | MACH_RCV_MSG,
        ::core::mem::size_of::<Req>() as mach_msg_size_t,
        ::core::mem::size_of::<Buf>() as mach_msg_size_t,
        reply_port,
        0,
        MACH_PORT_NULL,
    );

    // And dump the kr value so we know exactly what mach_msg returned.
    {
        let hexchars = b"0123456789abcdef";
        let mut line = [b'f', b's', b'y', b's', b'_', b's', b't', b'a', b'r', b't', b'u', b'p', b':', b' ', b'k', b'r', b'=', b'0', b'x',
                        b'0', b'0', b'0', b'0', b'0', b'0', b'0', b'0', b'\n'];
        let bytes = (ret as u32).to_be_bytes();
        for i in 0..4 {
            line[19 + i * 2]     = hexchars[(bytes[i] >> 4) as usize];
            line[19 + i * 2 + 1] = hexchars[(bytes[i] & 0x0f) as usize];
        }
        libc::write(2, line.as_ptr() as *const _, line.len());
    }
    if ret != KERN_SUCCESS {
        return ret;
    }

    let retcode = buf.rep.retcode.value;
    if retcode != 0 {
        return retcode;
    }
    *realnode = buf.rep.realnode.port_name;
    0
}

/// Standard translator bootstrap. Mirrors libtrivfs's `trivfs_startup`:
/// create a control port, send `fsys_startup` to the bootstrap port,
/// stash the returned underlying node.
///
/// `proc_mark_important` is skipped — it's a nice-to-have (tells proc
/// "don't kill this task"), not load-bearing for the rendezvous. Adding
/// it back is a few-line follow-up once we have a proc-sys binding.
#[no_mangle]
pub unsafe extern "C" fn trivfs_startup(
    bootstrap:      mach_port_t,
    flags:          c_int,
    control_class:  *mut port_class,
    control_bucket: *mut port_bucket,
    protid_class:   *mut port_class,
    protid_bucket:  *mut port_bucket,
    control:        *mut *mut TrivfsControl,
) -> error_t {
    let mut fsys: *mut TrivfsControl = null_mut();
    let err = create_control(
        MACH_PORT_NULL,
        control_class, control_bucket,
        protid_class,  protid_bucket,
        &mut fsys,
    );
    if err != 0 {
        return err;
    }

    // Hand the parent a send right on our control port and complete
    // the fsys_startup handshake.
    let right = ports_get_send_right(fsys as *mut c_void);
    let mut underlying: mach_port_t = 0;
    let kr = fsys_startup(bootstrap, flags, right, &mut underlying);
    mach_port_deallocate(mach_task_self(), right);

    if kr == 0 {
        (*fsys).underlying = underlying;
    }

    ports_port_deref(fsys as *mut c_void);

    if kr == 0 && !control.is_null() {
        *control = fsys;
    }

    kr
}
