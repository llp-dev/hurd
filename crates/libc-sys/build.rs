// Required by cargo because Cargo.toml declares `links = "c"`.
// We don't need to do any actual build-time work — the `#[link(name = "c")]`
// attribute in src/lib.rs already tells rustc to link libc. This file just
// exists to satisfy cargo's links-field requirement and reserve the unique
// libc-linkage slot in the dependency graph.

fn main() {
    // Intentionally empty.
}
