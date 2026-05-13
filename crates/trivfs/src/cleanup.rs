//! libports `clean_routine` callbacks for trivfs control and protid
//! port classes.
//!
//! libports invokes these with `void *port` pointing at the start of
//! the allocation (== `&port_info`). Because `port_info` is the first
//! field of both `TrivfsControl` and `TrivfsProtid`, the same pointer
//! cast to the trivfs type gives us the outer struct.
//!
//! For shutdown specifically, neither of these typically runs — the
//! translator either lives forever (until poweroff via acpi) or exits
//! abruptly via `libc::exit(0)` in `trivfs_goaway`, both of which skip
//! orderly port destruction. The callbacks exist so libports doesn't
//! crash with a NULL clean_routine if it ever does reach the dealloc
//! path (e.g., during a refcount drop on a transient protid).

use core::ffi::c_void;
use mach_sys::{mach_port_deallocate, mach_task_self, MACH_PORT_NULL};

use crate::types::{TrivfsControl, TrivfsProtid};

/// Clean a trivfs control port. The only owned resource is `underlying`
/// (the realnode mach port handed back by fsys_startup) — release it
/// so we don't leak port-name slots.
#[no_mangle]
pub unsafe extern "C" fn trivfs_clean_cntl(port: *mut c_void) {
    let fsys = port as *mut TrivfsControl;
    if (*fsys).underlying != MACH_PORT_NULL {
        mach_port_deallocate(mach_task_self(), (*fsys).underlying);
        (*fsys).underlying = MACH_PORT_NULL;
    }
}

/// Clean a trivfs protid. Releases the realnode reference. The peropen
/// has its own refcount — if this protid was the last reference to it,
/// the peropen would be freed too; for shutdown that path is unused.
#[no_mangle]
pub unsafe extern "C" fn trivfs_clean_protid(port: *mut c_void) {
    let cred = port as *mut TrivfsProtid;
    if (*cred).realnode != MACH_PORT_NULL {
        mach_port_deallocate(mach_task_self(), (*cred).realnode);
        (*cred).realnode = MACH_PORT_NULL;
    }
}
