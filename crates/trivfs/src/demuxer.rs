//! Incoming-message dispatch for trivfs translators.
//!
//! libports' `manage_port_operations_*` loop receives messages on a
//! bucket's portset and calls our demuxer for each one. Our job is to
//! pattern-match `msgh_id` and either handle the message (returning
//! nonzero) or pass it along (returning zero).
//!
//! Today this dispatch table is a stub: every msgh_id returns 0 (not
//! handled), which makes libports send back `MIG_BAD_ID` to the client.
//! That's enough for translator startup (`settrans -ac` doesn't send us
//! any RPCs — it just exec's us, expects us to call `fsys_startup`, and
//! then leaves). Any client that subsequently opens us will get
//! `MIG_BAD_ID` until handlers are wired up — one msgh_id at a time:
//!
//!   - 22001: fsys_goaway          (so `settrans -g` works)
//!   - 22002: fsys_getroot         (so clients can open the translator)
//!   - 21000..21099: io_*          (notably 21025 io_stat)
//!   - 20000..20099: file_/dir_    (most return EOPNOTSUPP)
//!
//! Until those are filled in, use the C `trivfs_demuxer` from
//! libtrivfs as a fallback. We do that by trying our own dispatch
//! first, then deferring; eventually we'll have full coverage and the
//! fallback goes away.

use libc::c_int;
use mach_sys::mach_msg_header_t;

/// Pure-Rust trivfs demuxer. Returns nonzero if the message was
/// handled (and the reply has been written into `outp`), 0 otherwise.
///
/// Currently always returns 0 — every msgh_id falls through to "not
/// handled". libports will send MIG_BAD_ID back to the client, which
/// is acceptable behavior for a translator that hasn't filled in any
/// handlers yet. Wire up handlers below.
#[no_mangle]
pub unsafe extern "C" fn trivfs_demuxer(
    _inp:  *mut mach_msg_header_t,
    _outp: *mut mach_msg_header_t,
) -> c_int {
    // TODO: dispatch by (*inp).msgh_id:
    //   match (*inp).msgh_id {
    //       22001 => handle_fsys_goaway(inp, outp),
    //       22002 => handle_fsys_getroot(inp, outp),
    //       21025 => handle_io_stat(inp, outp),
    //       _     => 0,
    //   }
    0
}
