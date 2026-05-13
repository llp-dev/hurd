// Stub. Will be replaced in Task 4 with the full port of init/init.rs.

fn main() {
    // Intentionally empty. Exists only so Task 3 can verify the [[bin]]
    // target and libc dependency are wired up correctly.
    let _ = libc::SIGCHLD; // ensure the dependency is actually used
}
