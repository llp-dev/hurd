// Required by cargo because Cargo.toml declares `links = "ports"`.
// #[link(name = "ports")] in src/lib.rs already tells rustc to link
// libports; this script just exists to satisfy cargo's links-field rule.

fn main() {}
