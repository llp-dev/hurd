//! POSIX / GNU-libc FFI declarations used across the Hurd Rust workspace.
//!
//! No code lives here, only `extern "C"` declarations, `#[repr(C)]` types,
//! constants, and small `#[inline]` macros translated from libc headers.
//!
//! This crate has no dependencies and is `no_std`-compatible so that future
//! `no_std` consumers (kernel-adjacent code) can use the same declarations.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]
