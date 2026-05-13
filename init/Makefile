#
#   Copyright (C) 2013 Free Software Foundation, Inc.
#
#   This program is free software; you can redistribute it and/or
#   modify it under the terms of the GNU General Public License as
#   published by the Free Software Foundation; either version 2, or (at
#   your option) any later version.
#
#   This program is distributed in the hope that it will be useful, but
#   WITHOUT ANY WARRANTY; without even the implied warranty of
#   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
#   General Public License for more details.
#
#   You should have received a copy of the GNU General Public License
#   along with this program.  If not, see <http://www.gnu.org/licenses/>.

dir := init
makemode := server

# init is implemented in Rust. See init.rs. The crate is #![no_std] +
# #![no_main] and uses FFI directly into glibc, so the final link looks
# exactly the same as the original C version did: crt1/crti come from libc,
# our `main` symbol comes from init.o, and libhurdbugaddr is linked in by
# the generic rules in ../Makeconf.

SRCS = init.rs
OBJS = init.o
target = init

# LIBEXECDIR and HURD_VERSION are baked into the Rust source via env!() at
# compile time (see uses of env!() in init.rs). HURD_VERSION mirrors what the
# C build does via STANDARD_HURD_VERSION / version.h.
RUSTFLAGS = \
	$(if $(rust_target),--target=$(rust_target)) \
	--edition=2021 \
	--crate-type=staticlib --emit=obj \
	-C panic=abort -C codegen-units=1 -C opt-level=3 -C debuginfo=2

init.o: init.rs
	LIBEXECDIR='$(libexecdir)' HURD_VERSION='$(package-version)' \
	  $(RUSTC) $(RUSTFLAGS) -o $@ $<

include ../Makeconf
