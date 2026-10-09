//! aphrody-libc: a Rust overlay for musl.
//!
//! musl stays the libc and the ABI (`libc.so`, `ld-musl-*.so.1`, every struct
//! layout). This crate only re-implements leaf functions whose musl versions
//! are known to be slow — byte-at-a-time `memcmp`/`strcmp`, word-at-a-time
//! `strlen`/`strchr`/`memchr`, smoothsort `qsort` — with identical C
//! signatures and semantics. They own no state, take no locks and never call
//! back into libc, so an overlay symbol can replace the musl one wherever the
//! linker or the dynamic loader picks it first:
//!
//! - `libaphrody_libc.a` before `-lc` (static replacement of the musl members);
//! - `libaphrody_libc.so` as `DT_NEEDED` before `libc.so`, or `LD_PRELOAD`.
//!
//! musl's own internal calls (printf, getaddrinfo, …) are bound inside
//! `libc.so` and keep using musl's versions; the overlay only serves callers
//! outside libc.
//!
//! `no_builtins` stops LLVM from turning the scalar tails below back into
//! calls to the very functions they implement (loop idiom recognition knows
//! `strlen`, `memset`, `memcpy`, `bcmp`).

#![cfg_attr(not(test), no_std)]
#![no_builtins]
#![allow(clippy::missing_safety_doc)]

pub mod qsort;
pub mod string;
mod simd;

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    // Nothing in the exported functions panics on valid input; reaching this
    // is a bug in the overlay, never something a C caller can recover from.
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    unsafe {
        core::arch::asm!("ud2", options(noreturn))
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("udf #0", options(noreturn))
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64")))]
    loop {}
}
