//! Hand-written Mach RPC marshalling for Hurd Rust servers.
//!
//! Instead of running the MIG tool against `.defs` files and linking the
//! generated C stubs, we marshall messages ourselves in Rust. This crate
//! provides:
//!
//! - `mach_reply_port()` re-export (allocates a fresh receive right per
//!   call — simpler than the MIG `mig_get_reply_port` thread-cached port
//!   and fine for low-frequency callers like a translator's shutdown
//!   handler).
//!
//! - `routine_call!` macro — generates a client stub for a routine that
//!   takes the server port as the first implicit argument, then N scalar
//!   in-arguments, and returns `kern_return_t`. The wire format matches
//!   GNU Mach's old-style protocol (NOT NDR): every inline scalar is
//!   preceded by a `mach_msg_type_t` descriptor.
//!
//! - `routine_serve!` macro — generates a server-side dispatch function
//!   that matches one msgh_id, decodes the request's tagged arguments,
//!   calls the user-supplied handler, and fills the reply with the
//!   returned `kern_return_t`.
//!
//! Both macros assume the "simple" MIG shape:
//!   - no port-right transfers (msgh_bits has no COMPLEX flag),
//!   - no out-of-line memory,
//!   - no variable-length arrays,
//!   - no out-arguments beyond the implicit `kern_return_t`.
//!
//! That's enough for `shutdown_shutdown` (inbound) and `acpi_sleep`
//! (outbound) and many similar small translators; richer shapes can be
//! added when needed.
//!
//! Wire format on x86_64-gnu (sizes in bytes):
//!
//! ```text
//! Request:  [mach_msg_header_t (32)] [Tagged<arg1> (16)] [Tagged<arg2> (16)] ...
//! Reply:    [mach_msg_header_t (32)] [Tagged<kern_return_t> (16)]
//! ```
//!
//! Each `Tagged<T>` is one `mach_msg_type_t` (8 bytes including 4-byte
//! alignment padding) followed by the value `T`, with the whole slot
//! padded to a multiple of 8 bytes by `repr(C, align(8))`. That keeps
//! the next descriptor 8-byte-aligned, matching what the MIG-generated
//! C code on the wire expects.

#![no_std]
#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

/// Re-export of mach-sys so macros can refer to it via `$crate::mach::...`
/// without forcing every consumer to add a `mach-sys` dependency separately.
pub use mach_sys as mach;

/// Re-export of the `routine!` proc-macro from `mig-macros`. End users
/// invoke it as `mig::routine! { ... }`.
pub use mig_macros::routine;

/// `mach_reply_port` is provided by `mach_sys` and re-exported here so
/// the macros can call it as `$crate::mach_reply_port` without an
/// explicit `mach-sys` import in every consumer.
pub use mach::mach_reply_port;

/// Client-side stub generator for a simple Mach RPC routine.
///
/// # Syntax
///
/// ```ignore
/// mig::routine_call! {
///     fn acpi_sleep(sleep_state: c_int) = 41000;
/// }
/// ```
///
/// Generates:
///
/// ```ignore
/// pub unsafe fn acpi_sleep(
///     server: mach_port_t,
///     sleep_state: c_int,
/// ) -> kern_return_t { ... }
/// ```
///
/// # Constraints
///
/// - Every argument type must implement `mach_sys::MigScalar`
///   (scalars only — port rights need a different macro shape).
/// - `msgh_id` is the constant from the `.defs` subsystem.
/// - The remote-port disposition is COPY_SEND, the reply-port
///   disposition is MAKE_SEND_ONCE — matches MIG's default.
/// - The reply-port receive right is leaked per call. For low-frequency
///   calls (shutdown!) this is fine; hot paths should use a cached port.
#[macro_export]
macro_rules! routine_call {
    (
        fn $fname:ident($($arg:ident: $ty:ty),* $(,)?) = $msgh_id:expr;
    ) => {
        #[allow(non_snake_case, non_camel_case_types, dead_code)]
        pub unsafe fn $fname(
            server: $crate::mach::mach_port_t,
            $($arg: $ty,)*
        ) -> $crate::mach::kern_return_t {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Req {
                head: $crate::mach::mach_msg_header_t,
                $($arg: $crate::mach::Tagged<$ty>,)*
            }
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Rep {
                head:    $crate::mach::mach_msg_header_t,
                retcode: $crate::mach::Tagged<$crate::mach::kern_return_t>,
            }
            #[repr(C)]
            union Buf { req: Req, rep: Rep }

            let reply_port = $crate::mach_reply_port();

            let mut buf = Buf {
                req: Req {
                    head: $crate::mach::mach_msg_header_t {
                        msgh_bits: $crate::mach::MACH_MSGH_BITS(
                            $crate::mach::MACH_MSG_TYPE_COPY_SEND,
                            $crate::mach::MACH_MSG_TYPE_MAKE_SEND_ONCE,
                        ),
                        msgh_size:        ::core::mem::size_of::<Req>() as u32,
                        msgh_remote_port: server,
                        _msgh_remote_pad: 0,
                        msgh_local_port:  reply_port,
                        _msgh_local_pad:  0,
                        msgh_seqno:       0,
                        msgh_id:          $msgh_id,
                    },
                    $($arg: $crate::mach::Tagged {
                        descriptor: <$ty as $crate::mach::MigScalar>::TYPE,
                        value:      $arg,
                    },)*
                },
            };

            let ret = $crate::mach::mach_msg(
                &mut buf.req.head as *mut _,
                $crate::mach::MACH_SEND_MSG | $crate::mach::MACH_RCV_MSG,
                ::core::mem::size_of::<Req>() as u32,
                ::core::mem::size_of::<Buf>() as u32,
                reply_port,
                0,
                $crate::mach::MACH_PORT_NULL,
            );
            if ret != $crate::mach::KERN_SUCCESS {
                return ret;
            }
            buf.rep.retcode.value
        }
    };
}

/// Server-side dispatcher generator for a simple Mach RPC routine.
///
/// # Syntax
///
/// ```ignore
/// mig::routine_serve! {
///     fn shutdown_server_routine for msgh_id 40000;
///     handler: my_shutdown_handler() -> kern_return_t;
/// }
/// ```
///
/// Generates:
///
/// ```ignore
/// unsafe extern "C" fn shutdown_server_routine(
///     inp:  *mut mach_msg_header_t,
///     outp: *mut mach_msg_header_t,
/// ) -> c_int { ... }
/// ```
///
/// The dispatcher checks `(*inp).msgh_id == $msgh_id`. If it matches,
/// it decodes the tagged arguments from the request, calls
/// `my_shutdown_handler(server_port, arg1, arg2, ...)`, writes the
/// returned `kern_return_t` into the reply, and returns 1. If it
/// doesn't match, returns 0 (giving the caller a chance to try another
/// demuxer like `trivfs_demuxer`).
#[macro_export]
macro_rules! routine_serve {
    (
        fn $fname:ident for msgh_id $msgh_id:expr;
        handler: $handler:ident($($arg:ident: $ty:ty),* $(,)?) -> $crty:ty;
    ) => {
        #[allow(non_snake_case, non_camel_case_types, dead_code)]
        unsafe extern "C" fn $fname(
            inp:  *mut $crate::mach::mach_msg_header_t,
            outp: *mut $crate::mach::mach_msg_header_t,
        ) -> ::core::ffi::c_int {
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Req {
                head: $crate::mach::mach_msg_header_t,
                $($arg: $crate::mach::Tagged<$ty>,)*
            }
            #[repr(C)]
            #[derive(Copy, Clone)]
            struct Rep {
                head:    $crate::mach::mach_msg_header_t,
                retcode: $crate::mach::Tagged<$crate::mach::kern_return_t>,
            }

            if (*inp).msgh_id != $msgh_id { return 0; }

            let req: &Req = &*(inp as *const Req);
            let rep: &mut Rep = &mut *(outp as *mut Rep);
            let server_port = req.head.msgh_local_port;

            $(let $arg = req.$arg.value;)*
            let retcode: $crty = $handler(server_port $(, $arg)*);

            rep.head.msgh_bits = $crate::mach::MACH_MSGH_BITS(
                $crate::mach::MACH_MSGH_BITS_REMOTE(req.head.msgh_bits),
                0,
            );
            rep.head.msgh_size        = ::core::mem::size_of::<Rep>() as u32;
            rep.head.msgh_remote_port = req.head.msgh_remote_port;
            rep.head._msgh_remote_pad = 0;
            rep.head.msgh_local_port  = 0;
            rep.head._msgh_local_pad  = 0;
            rep.head.msgh_seqno       = 0;
            // MIG reply convention: response msgh_id is request + 100.
            rep.head.msgh_id          = req.head.msgh_id + 100;

            rep.retcode = $crate::mach::Tagged {
                descriptor: <$crate::mach::kern_return_t as $crate::mach::MigScalar>::TYPE,
                value:      retcode,
            };

            1
        }
    };
}
