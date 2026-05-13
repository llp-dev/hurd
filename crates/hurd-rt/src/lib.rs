//! Minimalist no_std runtime for Hurd userspace binaries.
//!
//! Provides the boilerplate every cargo-built Hurd binary needs:
//!
//! 1. The `main!` macro hides the `#[no_mangle] pub unsafe extern "C" fn main`
//!    declaration. User code declares `hurd_rt::main!(|argc, argv| { ... })`
//!    and the macro generates the C-ABI entry point that crt1 calls.
//!
//! 2. A default `#[panic_handler]` that aborts. Gated on `#[cfg(not(test))]`
//!    so `cargo check --tests` doesn't see a duplicate definition against
//!    libstd's panic_impl.
//!
//! Modeled on `cortex-m-rt` for embedded ARM and similar runtime crates in
//! the no_std ecosystem.

#![no_std]

// Re-export the C types our `main!` macro names in its expansion, so users
// don't have to add their own `use libc::{c_char, c_int};` line.
pub use libc::{c_char, c_int};

/// Declare a Hurd binary entry point.
///
/// Expands to a `#[no_mangle] pub unsafe extern "C" fn main(argc, argv)` that
/// crt1 calls after the C runtime has set up the address space. The body has
/// access to `argc` and `argv` under whatever names you bind them to.
///
/// # Example
///
/// ```ignore
/// hurd_rt::main!(|argc, argv| {
///     // ... your code here ...
///     0  // exit status
/// });
/// ```
#[macro_export]
macro_rules! main {
    (| $argc:ident, $argv:ident | $body:block) => {
        #[no_mangle]
        pub unsafe extern "C" fn main(
            $argc: $crate::c_int,
            $argv: *mut *mut $crate::c_char,
        ) -> $crate::c_int {
            $body
        }
    };
}

/// Default panic handler — aborts the process.
///
/// init and other PID-1-class binaries never panic intentionally; if they
/// somehow do, calling glibc's `abort()` is the most honest outcome (a Rust
/// panic in a critical server is no worse than a crash). With
/// `panic = "abort"` in the release profile, no unwinding tables are linked,
/// so this handler is fundamentally a one-liner.
///
/// `#[cfg(not(test))]` keeps rust-analyzer / `cargo check --tests` quiet:
/// in test mode the libstd panic_impl is in scope and would clash.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { libc::abort() }
}
