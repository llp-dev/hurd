// Compile-only smoke test for mig::routine!. Exercises the full type-
// tag vocabulary in a single fictional RPC. Build with:
//
//   cargo build --example smoketest -p mig-macros
//
// Until Task 9 lands, this is expected to fail compilation. After
// Task 9 it should compile cleanly.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use mach_sys::{kern_return_t, mach_port_t};

mig_macros::routine! {
    fn fsys_startup = 22000;
    in {
        target:        mach_port_t;
        openflags:     int;
        control_port:  port_send_poly;
    }
    out {
        realnode:      port_send;
    }
}

fn main() {
    let _ = fsys_startup;  // just confirm the symbol exists
}
