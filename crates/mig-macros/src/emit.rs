//! Codegen for `routine!`. Takes a `ParsedRoutine` and emits a Rust
//! source string, then parses it back into a `TokenStream`.
//!
//! Why source-string rather than syn/quote: every macro invocation is
//! visible here, the grammar is rigid, and the dependency cost of syn
//! is real. The format!()-based approach is debuggable (print the
//! string!) and stays under 200 lines.
//!
//! Generated code uses absolute `::mach_sys::...` paths. Consumers
//! must depend on the `mach-sys` crate (transitively via `mig` is
//! fine — mig re-exports the macro but the paths inside resolve to
//! mach-sys directly to avoid a cycle when mig-macros itself is the
//! callsite).

use proc_macro::TokenStream;

use crate::parse::{Arg, ParsedRoutine, TypeTag};

pub fn emit(p: &ParsedRoutine) -> TokenStream {
    let fname    = &p.fname;
    let msgh_id  = p.msgh_id;
    let reply_id = msgh_id + 100;

    // ---- function signature ----
    let mut sig_args = String::new();
    sig_args.push_str(&format!("{}: ::mach_sys::mach_port_t, ", p.target));
    for arg in &p.in_args {
        sig_args.push_str(&fn_param(arg));
    }
    for arg in &p.out_args {
        sig_args.push_str(&format!("{}: *mut {}, ", arg.name, rust_out_ty(arg.tag)));
    }

    // ---- Request struct fields ----
    let mut req_fields = String::new();
    for arg in &p.in_args {
        req_fields.push_str(&req_field(arg));
    }

    // ---- Reply struct fields ----
    let mut rep_fields = String::new();
    for arg in &p.out_args {
        rep_fields.push_str(&rep_field(arg));
    }

    // ---- per-arg request field writes ----
    let mut req_writes = String::new();
    for arg in &p.in_args {
        req_writes.push_str(&req_write(arg));
    }

    // ---- complex-bit decision ----
    let any_in_port = p.in_args.iter().any(|a| is_port(a.tag));
    let complex_in  = if any_in_port { "::mach_sys::MACH_MSGH_BITS_COMPLEX" } else { "0" };

    // ---- the whole emitted function ----
    let src = format!(r####"
        #[allow(non_snake_case, non_camel_case_types, dead_code, unused_unsafe, unused_mut, unused_variables)]
        pub unsafe fn {fname}({sig_args}) -> ::mach_sys::kern_return_t {{
            use ::mach_sys::*;

            #[repr(C)]
            struct Request {{
                head: mach_msg_header_t,
                {req_fields}
            }}

            #[repr(C)]
            struct Reply {{
                head:         mach_msg_header_t,
                retcode_type: mach_msg_type_t,
                retcode:      kern_return_t,
                retcode_pad:  u32,
                {rep_fields}
            }}

            #[repr(C)]
            union Mess {{
                in_: ::core::mem::ManuallyDrop<Request>,
                out: ::core::mem::ManuallyDrop<Reply>,
            }}

            const REQ_SIZE:   mach_msg_size_t = ::core::mem::size_of::<Request>() as mach_msg_size_t;
            const REPLY_SIZE: mach_msg_size_t = ::core::mem::size_of::<Reply>()   as mach_msg_size_t;
            const REQUEST_ID: mach_msg_id_t   = {msgh_id};
            const REPLY_ID:   mach_msg_id_t   = {reply_id};

            // Zero the whole storage so any inter-field padding stays
            // zero on the wire (Rust struct-literal init does NOT
            // guarantee this; raw-pointer writes preserve the zeroed
            // padding).
            let mut storage = ::core::mem::MaybeUninit::<Mess>::zeroed();
            let inp = storage.as_mut_ptr() as *mut Request;

            // ---- per-arg field writes ----
            {req_writes}

            // ---- header ----
            (*inp).head.msgh_bits         = {complex_in}
                | MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE);
            (*inp).head.msgh_size         = REQ_SIZE;
            (*inp).head.msgh_remote_port  = {target};
            (*inp).head._msgh_remote_pad  = 0;
            (*inp).head.msgh_local_port   = mig_get_reply_port();
            (*inp).head._msgh_local_pad   = 0;
            (*inp).head.msgh_seqno        = 0;
            (*inp).head.msgh_id           = REQUEST_ID;

            let reply_port = (*inp).head.msgh_local_port;

            let kr = mach_msg(
                storage.as_mut_ptr() as *mut mach_msg_header_t,
                MACH_SEND_MSG | MACH_RCV_MSG,
                REQ_SIZE, REPLY_SIZE,
                reply_port,
                0,
                MACH_PORT_NULL,
            );
            if kr != KERN_SUCCESS {{
                mig_dealloc_reply_port(reply_port);
                return kr;
            }}
            mig_put_reply_port(reply_port);

            // Reply validation + extraction lands in Task 9.
            let _ = REPLY_ID;
            let _ = (REQUEST_ID, REPLY_SIZE);
            KERN_SUCCESS
        }}
    "####,
        fname     = fname,
        sig_args  = sig_args,
        req_fields= req_fields,
        rep_fields= rep_fields,
        req_writes= req_writes,
        complex_in= complex_in,
        target    = p.target,
        msgh_id   = msgh_id,
        reply_id  = reply_id,
    );

    src.parse().expect("emit produced invalid Rust")
}

// ---- per-arg fragments ----

fn fn_param(arg: &Arg) -> String {
    match arg.tag {
        TypeTag::Int            => format!("{}: ::mach_sys::c_int, ", arg.name),
        TypeTag::PortSend       => format!("{}: ::mach_sys::mach_port_t, ", arg.name),
        TypeTag::PortSendPoly   => format!(
            "{n}: ::mach_sys::mach_port_t, {n}Poly: ::mach_sys::c_int, ",
            n = arg.name,
        ),
        TypeTag::MachPortT      => unreachable!("target arg handled separately"),
    }
}

fn rust_out_ty(tag: TypeTag) -> &'static str {
    match tag {
        TypeTag::Int            => "::mach_sys::c_int",
        TypeTag::PortSend       => "::mach_sys::mach_port_t",
        TypeTag::PortSendPoly   => "::mach_sys::mach_port_t",
        TypeTag::MachPortT      => unreachable!(),
    }
}

fn req_field(arg: &Arg) -> String {
    match arg.tag {
        TypeTag::Int => format!(
            "{n}_type: mach_msg_type_t, {n}: ::mach_sys::c_int, {n}_pad: u32,\n",
            n = arg.name,
        ),
        TypeTag::PortSend | TypeTag::PortSendPoly => format!(
            "{n}_type: mach_msg_type_t, {n}: mach_port_name_inlined_t,\n",
            n = arg.name,
        ),
        TypeTag::MachPortT => unreachable!(),
    }
}

fn rep_field(arg: &Arg) -> String {
    match arg.tag {
        TypeTag::Int => format!(
            "{n}_type: mach_msg_type_t, {n}: ::mach_sys::c_int, {n}_pad: u32,\n",
            n = arg.name,
        ),
        TypeTag::PortSend | TypeTag::PortSendPoly => format!(
            "{n}_type: mach_msg_type_t, {n}: mach_port_name_inlined_t,\n",
            n = arg.name,
        ),
        TypeTag::MachPortT => unreachable!(),
    }
}

fn req_write(arg: &Arg) -> String {
    match arg.tag {
        TypeTag::Int => format!(
            "(*inp).{n}_type = MIG_TYPE_INT32; (*inp).{n} = {n};\n",
            n = arg.name,
        ),
        TypeTag::PortSend => format!(
            "(*inp).{n}_type = MIG_TYPE_PORT_COPY_SEND; (*inp).{n}.name = {n};\n",
            n = arg.name,
        ),
        TypeTag::PortSendPoly => format!(
            "\
            (*inp).{n}_type = MIG_TYPE_PORT_SEND_POLY;\n\
            (*inp).{n}.name = {n};\n\
            // Replace the low-8-bit msgt_name with the caller's disposition.\n\
            (*inp).{n}_type.bits = ({n}Poly as u32 & 0xff) | ((*inp).{n}_type.bits & !0xff);\n",
            n = arg.name,
        ),
        TypeTag::MachPortT => unreachable!(),
    }
}

fn is_port(t: TypeTag) -> bool {
    matches!(t, TypeTag::PortSend | TypeTag::PortSendPoly)
}
