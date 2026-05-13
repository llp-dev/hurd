//! Minimalist no_std runtime for Hurd userspace binaries.
//!
//! Provides the entry-point + panic-handler boilerplate every cargo-built
//! no_std Hurd binary needs:
//!
//! ```ignore
//! use hurd_rt::{c_char, c_int};
//!
//! #[hurd_rt::entry]
//! fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
//!     // ... your code ...
//!     0
//! }
//! ```
//!
//! `#[hurd_rt::entry]` is a proc-macro attribute that expands to the
//! `#[no_mangle] pub unsafe extern "C" fn main(...)` declaration that
//! crt1 invokes. The default `#[panic_handler]` (below) aborts via
//! `libc::abort()` and is `#[cfg(not(test))]`-gated so rust-analyzer's
//! `--tests` check doesn't see a duplicate against libstd's panic_impl.
//!
//! Modeled on cortex-m-rt's `#[entry]` and similar no_std runtime crates.

#![no_std]

// Re-export the proc-macro attribute so end users write
// `#[hurd_rt::entry]`, not `#[hurd_rt_macros::entry]`.
pub use hurd_rt_macros::entry;

// Re-export the C types the `#[entry]`-decorated function will name in
// its signature, so users don't need a separate `use libc::{c_char, c_int};`.
pub use libc::{c_char, c_int};

/// Default panic handler — aborts the process.
///
/// init and other PID-1-class binaries never panic intentionally; if they
/// somehow do, calling glibc's `abort()` is the most honest outcome (a Rust
/// panic in a critical server is no worse than a crash). With
/// `panic = "abort"` in the release profile, no unwinding tables are linked,
/// so this handler is fundamentally a one-liner.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { libc::abort() }
}
