//! Procedural macros for the hurd-rt runtime crate.
//!
//! Currently provides one attribute: `#[entry]`, which decorates the
//! user's `fn main` so the produced symbol matches the C ABI that crt1
//! invokes.
//!
//! Uses only the standard `proc_macro` API — no `syn`/`quote` dependency,
//! keeping the workspace external-crate-free.

use proc_macro::TokenStream;

/// Mark a function as the binary's entry point.
///
/// The user writes a plain Rust function named `main` taking
/// `(c_int, *mut *mut c_char)` and returning `c_int`. The attribute
/// prepends `#[no_mangle] pub unsafe extern "C"` so the symbol the
/// C runtime expects is produced.
///
/// # Example
///
/// ```ignore
/// use hurd_rt::{c_char, c_int};
///
/// #[hurd_rt::entry]
/// fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
///     // ... your code ...
///     0
/// }
/// ```
///
/// Restrictions:
/// - The function must be named `main` (the C ABI requires this symbol).
/// - The function must not carry `pub`, `unsafe`, or `extern "..."`
///   qualifiers in the source — the attribute adds them.
#[proc_macro_attribute]
pub fn entry(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // The simplest valid implementation: prepend the qualifiers the
    // C entry point needs, then emit the user's function body as-is.
    let prefix: TokenStream = r#"#[no_mangle] pub unsafe extern "C" "#
        .parse()
        .expect("hurd-rt-macros: failed to parse prefix");
    let mut output = prefix;
    output.extend(item);
    output
}
