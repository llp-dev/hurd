// Required by cargo because Cargo.toml declares `links = "trivfs"`.
// #[link(name = "trivfs")] in src/lib.rs already tells rustc to link
// libtrivfs; this script just exists to satisfy cargo's links-field rule.

fn main() {}
