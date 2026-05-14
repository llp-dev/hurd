# Pure-Rust MIG Macro for Port-Transferring RPCs — Design Spec

**Status:** Approved 2026-05-14
**Author:** rust-port working session
**Supersedes parts of:** the empirical `extern "C" fn fsys_startup` workaround in `crates/trivfs/src/startup.rs` (commit 0983882b)

## Problem

The Rust trivfs port currently calls libhurduser's mig-generated `fsys_startup` via `extern "C"` because our hand-rolled marshalling produces `MACH_SEND_INVALID_TYPE` (`0x1000000f`). Replacing libhurduser with pure Rust is the next milestone for the trivfs work and a prerequisite for every translator port that transfers port rights (which is nearly all of them).

The existing `mig::routine_call!` and `mig::routine_serve!` declarative macros only handle the all-scalar case. They don't know how to:
- Emit port-descriptor wire bytes (`MACH_MSG_TYPE_PORT_SEND`, `MACH_MSG_TYPE_POLYMORPHIC`)
- Set the `MACH_MSGH_BITS_COMPLEX` flag on the header when any out-of-line port-right transfer is present
- Validate reply descriptors against expected types
- Handle the "short error reply" path where a server errors out before allocating return ports

## Root Cause of the Bug (Historical Context)

The hand-roll failed because `mach_msg_type_t` has a **different C struct layout** on `__LP64__` than on i386, and our `mach-sys` crate was using the i386 layout. On x86_64 GNU Mach:

```c
typedef struct {
    unsigned int  msgt_name : 8,        // bits  0..7
                  msgt_size : 16,       // bits  8..23
                  msgt_unused : 5,      // bits 24..28
                  msgt_inline : 1,      // bit   29
                  msgt_longform : 1,    // bit   30
                  msgt_deallocate : 1;  // bit   31
    mach_msg_type_number_t msgt_number;  // separate u32, bits 32..63
} __attribute__((aligned(__alignof__(uintptr_t)))) mach_msg_type_t;
```

Our old encoding placed `msgt_inline` at bit 28, which on LP64 falls inside the `msgt_unused` field. The kernel checks `if (msgt_unused != 0) return MACH_SEND_INVALID_TYPE` and rejected every message. Diagnostically invisible to a hex dump of the *bytes* — only visible by comparing field-by-field against a working mig stub.

The fix is a redesign of `mach_msg_type_t` to two explicit `u32` fields with named bit positions, plus a macro that lets us build correct wire bytes once and reuse them per-routine.

## Design

### 1. Macro Syntax

```rust
mig::routine! {
    fn fsys_startup = 22000;
    in {
        target:               mach_port_t;
        openflags:            int;
        control_port:         port_send_poly;
    }
    out {
        realnode:             port_send;
    }
}
```

Grammar:
```
routine!     := "fn" IDENT "=" INT ";" "in" "{" arg_list "}" "out" "{" arg_list "}"
arg_list     := ( IDENT ":" type_tag ";" )*
type_tag     := "int" | "port_send" | "port_send_poly" | "mach_port_t"
```

`target: mach_port_t` is special: it identifies the request port (the destination of the RPC) and is **not** placed in the message body. Exactly one `target` is required per `in` block.

The first arg in `in` after `target` is the first wire-format arg; subsequent args follow declaration order.

### 2. Type Tags

Initial vocabulary (extensible later without breaking the macro grammar):

| Tag | Rust ABI | Wire slot size | Descriptor |
|---|---|---|---|
| `int` | `c_int` | 8 bytes (4 value + 4 pad) | `MIG_TYPE_INT32` |
| `port_send` | `mach_port_t` | 8 bytes (`mach_port_name_inlined_t`) | `MIG_TYPE_PORT_SEND` |
| `port_send_poly` | `mach_port_t` (+ `xxxPoly: c_int`) | 8 bytes | `MIG_TYPE_PORT_SEND_POLY` (caller-overridden `msgt_name`) |
| `mach_port_t` (in-position 1 only) | `mach_port_t` | n/a — goes in `msgh_request_port` | n/a |

`port_send_poly` expands to **two** parameters in the generated function signature: the port name and a `c_int` disposition (e.g. `MACH_MSG_TYPE_COPY_SEND`, `MACH_MSG_TYPE_MAKE_SEND`). Matches mig's `polymorphic` convention.

### 3. Wire-Format Types (LP64)

Lives in `crates/mig/src/wire.rs`, re-exported as `mig::wire`.

```rust
#[repr(C, align(8))]
pub struct mach_msg_type_t {
    pub bits:   u32,  // name:8 | size:16 | unused:5 | inline:1 | longform:1 | deallocate:1
    pub number: u32,
}

const _: () = assert!(core::mem::size_of::<mach_msg_type_t>() == 8);

// Bit accessors are const fns so descriptors can be evaluated at compile time.
const fn mig_type(name: u8, size_bits: u16, inline: bool) -> mach_msg_type_t {
    let bits = (name as u32)
             | ((size_bits as u32) << 8)
             | ((inline as u32) << 29);
    mach_msg_type_t { bits, number: 1 }
}

pub const MIG_TYPE_INT32:           mach_msg_type_t = mig_type(MACH_MSG_TYPE_INTEGER_32, 32, true);
pub const MIG_TYPE_PORT_SEND:       mach_msg_type_t = mig_type(MACH_MSG_TYPE_PORT_SEND, 64, true);
pub const MIG_TYPE_PORT_SEND_POLY:  mach_msg_type_t = mig_type(MACH_MSG_TYPE_POLYMORPHIC, 64, true);
```

`mach_port_name_inlined_t` matches the kernel union (8 bytes on LP64):

```rust
#[repr(C)]
pub union mach_port_name_inlined_t {
    pub name:                    mach_port_t,
    pub kernel_port_do_not_use:  usize,
}
```

`mach-sys`'s existing `mach_msg_type_t` is **replaced** by this layout. Any code currently using the old encoding (the declarative `routine_call!`/`routine_serve!`) must be updated — those are the only call sites and they're in this workspace.

### 4. Generated Code Shape

For the running `fsys_startup` example, the macro emits:

```rust
pub unsafe fn fsys_startup(
    target:           mach_port_t,
    openflags:        c_int,
    control_port:     mach_port_t,
    control_portPoly: c_int,
    realnode:         *mut mach_port_t,
) -> kern_return_t {
    use ::mig::wire::*;

    #[repr(C)]
    struct Request {
        head:             mach_msg_header_t,
        openflags_type:   mach_msg_type_t,
        openflags:        c_int,
        openflags_pad:    u32,
        control_port_type: mach_msg_type_t,
        control_port:     mach_port_name_inlined_t,
    }

    #[repr(C)]
    struct Reply {
        head:             mach_msg_header_t,
        retcode_type:     mach_msg_type_t,
        retcode:          kern_return_t,
        retcode_pad:      u32,
        realnode_type:    mach_msg_type_t,
        realnode:         mach_port_name_inlined_t,
    }

    #[repr(C)]
    union Mess { in_: core::mem::ManuallyDrop<Request>, out: core::mem::ManuallyDrop<Reply> }

    const REQ_SIZE: usize = core::mem::size_of::<Request>();
    const REPLY_SIZE: usize = core::mem::size_of::<Reply>();
    const REQUEST_ID: mach_msg_id_t = 22000;
    const REPLY_ID:   mach_msg_id_t = 22100;

    // MaybeUninit::zeroed → wire-bytes are 0 by default; padding stays zero.
    let mut storage = core::mem::MaybeUninit::<Mess>::zeroed();
    let inp = storage.as_mut_ptr() as *mut Request;

    // Field writes via raw pointer to avoid Rust's UB rules around
    // struct-literal padding.
    (*inp).openflags_type    = MIG_TYPE_INT32;
    (*inp).openflags         = openflags;
    (*inp).control_port_type = MIG_TYPE_PORT_SEND_POLY;
    (*inp).control_port.name = control_port;
    // Caller-supplied disposition overrides msgt_name:
    (*inp).control_port_type.bits =
        (control_portPoly as u32 & 0xff)
        | ((*inp).control_port_type.bits & !0xff);

    // Complex bit if any port descriptor; here yes.
    let msgh_simple = !MACH_MSG_TYPE_PORT_ANY(control_portPoly);
    let complex_bit = if msgh_simple { 0 } else { MACH_MSGH_BITS_COMPLEX };

    (*inp).head.msgh_bits = complex_bit
        | MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, MACH_MSG_TYPE_MAKE_SEND_ONCE);
    (*inp).head.msgh_size = REQ_SIZE as mach_msg_size_t;
    (*inp).head.msgh_request_port.name = target;
    (*inp).head.msgh_reply_port.name = mig_get_reply_port();
    (*inp).head.msgh_seqno = 0;
    (*inp).head.msgh_id    = REQUEST_ID;

    let kr = mach_msg(
        storage.as_mut_ptr() as *mut mach_msg_header_t,
        MACH_SEND_MSG | MACH_RCV_MSG,
        REQ_SIZE as mach_msg_size_t,
        REPLY_SIZE as mach_msg_size_t,
        (*inp).head.msgh_reply_port.name,
        MACH_MSG_TIMEOUT_NONE,
        MACH_PORT_NULL,
    );

    if kr != MACH_MSG_SUCCESS {
        mig_dealloc_reply_port((*inp).head.msgh_reply_port.name);
        return kr;
    }
    mig_put_reply_port((*inp).head.msgh_reply_port.name);

    let outp = storage.as_mut_ptr() as *mut Reply;

    // ── Reply validation ──
    if (*outp).head.msgh_id != REPLY_ID {
        if (*outp).head.msgh_id == MACH_NOTIFY_SEND_ONCE {
            return MIG_SERVER_DIED;
        }
        return MIG_REPLY_MISMATCH;
    }

    let msgh_size   = (*outp).head.msgh_size as usize;
    let msgh_simple = ((*outp).head.msgh_bits & MACH_MSGH_BITS_COMPLEX) == 0;

    // Full reply or short error reply only.
    let full_ok  = msgh_size == REPLY_SIZE && !msgh_simple;
    let short_ok = msgh_size == core::mem::size_of::<mig_reply_header_t>()
                   && msgh_simple
                   && (*outp).retcode != KERN_SUCCESS;
    if !full_ok && !short_ok {
        return MIG_TYPE_ERROR;
    }

    if bad_typecheck(&(*outp).retcode_type, &MIG_TYPE_INT32) {
        return MIG_TYPE_ERROR;
    }
    if (*outp).retcode != KERN_SUCCESS {
        return (*outp).retcode;
    }
    if bad_typecheck(&(*outp).realnode_type, &MIG_TYPE_PORT_SEND) {
        return MIG_TYPE_ERROR;
    }

    *realnode = (*outp).realnode.name;
    KERN_SUCCESS
}
```

`bad_typecheck` is an inline helper in `mig::wire`:

```rust
#[inline(always)]
pub unsafe fn bad_typecheck(a: *const mach_msg_type_t, b: *const mach_msg_type_t) -> bool {
    core::ptr::read(a as *const u64) != core::ptr::read(b as *const u64)
}
```

Safe on x86_64 LP64 little-endian (always the case for GNU Mach on this target).

### 5. Crate Organization

```
crates/
├── mig/                 — existing crate, restructured
│   ├── Cargo.toml       — adds dep on mig-macros
│   ├── src/lib.rs       — re-exports mig_macros::routine; keeps old routine_call!/routine_serve!
│   └── src/wire.rs      — NEW: mach_msg_type_t (LP64), MIG_TYPE_* consts, bad_typecheck,
│                          mig_get_reply_port/mig_put_reply_port/mig_dealloc_reply_port
└── mig-macros/          — NEW proc-macro crate
    ├── Cargo.toml       — proc-macro = true, no syn/quote
    └── src/lib.rs       — parses routine! tokens, emits stub via TokenStream::from_str
```

No external dependencies (syn/quote). Matches the existing `hurd-rt-macros` pattern.

### 6. mach-sys Changes

`crates/mach-sys/src/lib.rs`:
- **Remove** the old `mach_msg_type_t` and `MIG_TYPE_INT32` const (wrong layout)
- The new types live in `mig::wire` (the higher-level crate), not `mach-sys`. `mach-sys` should only export raw kernel types, and `mach_msg_type_t` is straddling — it's a kernel type, but with mig-specific bit conventions. Putting it in `mig::wire` keeps `mach-sys` focused on `mach_msg.h`-level primitives.
- Re-export `mach_msg_type_t` from `mig::wire` if any non-mig caller needs it (unlikely).

Update the declarative `routine_call!`/`routine_serve!` to import from `mig::wire`. These are scalar-only and were never affected by the LP64 bug (no port descriptors), but the type definition must be consistent across the workspace.

### 7. Migration of `trivfs::startup`

Replace:
```rust
extern "C" {
    fn fsys_startup(...);
}
```

With:
```rust
mig::routine! {
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
```

Remove the `-lhurduser` link flag from `.cargo/config.toml`. Verify by running `settrans -ac /tmp/testshutdown ./target/release/shutdown` and confirming `stat` still reports the trivfs as `FSTYPE_MISC`.

## Non-Goals

- **No out-of-line data (OOL).** mig supports OOL arrays via `MACH_MSG_TYPE_MOVE_SEND_OOL` etc.; we don't need them yet. Adding them is a future tag.
- **No string types.** Same reason.
- **No simpleroutines** (`simpleroutine` in mig = no reply). The macro always generates the send+receive pair. Can be added with an `out {}` empty block + a syntactic marker.
- **No struct-typed args.** Every arg is a scalar or a port. Compound types come later if needed.
- **Not replacing the server side (`routine_serve!`).** The existing declarative macro handles trivfs/shutdown's inbound dispatch fine. A server-side `routine!` could come later but is out of scope for this milestone.

## Testing

Manual end-to-end on the Debian/Hurd VM:

1. `cargo build --release -p shutdown` — compiles clean
2. `settrans -ac /tmp/testshutdown ./target/release/shutdown` — attaches successfully
3. `stat /tmp/testshutdown` — reports character special, FSTYPE_MISC
4. Binary symbol check: `nm target/release/shutdown | grep fsys_startup` — should resolve to our generated code, **not** libhurduser

Compile-time checks (via `const _: () = assert!(...)` in `mig::wire`):
- `size_of::<mach_msg_type_t>() == 8`
- `size_of::<mach_msg_header_t>() == 32`
- `size_of::<mach_port_name_inlined_t>() == 8`

These will fail the build immediately if the LP64 assumption is ever broken (e.g. someone trying to cross-compile to 32-bit Hurd).

## Open Questions (Deferred)

- **Server-side macro for port-transferring inbound RPCs.** trivfs's `fsys_getroot`/`io_stat` return ports; once those handlers exist we'll need the reply-side equivalent. Punt until the demuxer milestone.
- **Generated function name collisions.** If two `routine!` invocations in the same module both name `fsys_startup`, Rust catches it. Cross-module is fine. No mangling needed.

## Related Memories

- `[[project-rust-trivfs-milestone]]` — context on what's working now
- `[[project-rust-toolchain]]` — native rustc 1.94.1 on Hurd, no cross-compile assumptions
