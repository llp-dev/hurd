//! Pure-Rust MIG client-stub generator for port-transferring RPCs.
//!
//! Entry point: the `routine!` function-like proc-macro. Grammar:
//!
//! ```ignore
//! mig::routine! {
//!     fn fsys_startup = 22000;
//!     in {
//!         target:        mach_port_t;
//!         openflags:     int;
//!         control_port:  port_send_poly;
//!     }
//!     out {
//!         realnode:      port_send;
//!     }
//! }
//! ```
//!
//! Generates a `pub unsafe fn fsys_startup(...) -> kern_return_t` that
//! marshalls the request, sends + receives via `mach_msg`, validates
//! the reply, and extracts out-args.
//!
//! Wire layout follows GNU Mach's old MIG protocol (NOT NDR) on x86_64
//! LP64. See `docs/superpowers/specs/2026-05-14-mig-port-transfer-macro-design.md`
//! for the full layout reference.
//!
//! Implementation note: this crate intentionally avoids syn/quote. The
//! grammar is rigid and we control every call site, so a small hand-
//! written TokenStream walker + `format!`-based emission is plenty
//! and keeps the build tree light.

extern crate proc_macro;

mod parse;
mod emit;

use proc_macro::TokenStream;

#[proc_macro]
pub fn routine(input: TokenStream) -> TokenStream {
    match parse::parse(input) {
        Ok(p) => emit::emit(&p),
        Err(ts) => ts,
    }
}
