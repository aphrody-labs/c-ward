//! 16-byte vectors with a lane mask: SSE2 on x86_64 (baseline of the
//! architecture), NEON on aarch64 (baseline too), a scalar stand-in elsewhere.
//!
//! `mask()` sets `STRIDE` bits per lane whose bytes are all ones (the result of
//! `eq`); the first matching lane is `trailing_zeros / STRIDE`, the last is
//! `(63 - leading_zeros) / STRIDE`. `ALL` is the mask of a vector where every
//! lane matches.

pub const W: usize = 16;

/// True when a 16-byte load at `p` would touch the next 4 KiB page. Loads that
/// stay inside the page of a valid byte cannot fault, which is what lets the
/// NUL-terminated scans read ahead of the terminator.
#[inline(always)]
pub fn crosses_page(p: *const u8) -> bool {
    (p as usize & 4095) > 4096 - W
}

/// Mask with the low `bits` bits set (saturating at 64).
#[inline(always)]
pub fn low_bits(bits: usize) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// Lanes `[lo, hi)` of a mask.
#[inline(always)]
pub fn lanes(lo: usize, hi: usize) -> u64 {
    low_bits(hi * STRIDE) & !low_bits(lo * STRIDE)
}

#[inline(always)]
pub fn first(m: u64) -> usize {
    m.trailing_zeros() as usize / STRIDE
}

#[inline(always)]
pub fn last(m: u64) -> usize {
    (63 - m.leading_zeros() as usize) / STRIDE
}

#[cfg(target_arch = "x86_64")]
mod imp {
    use core::arch::x86_64::*;

    pub const STRIDE: usize = 1;
    pub const ALL: u64 = 0xffff;

    #[derive(Clone, Copy)]
    pub struct V(__m128i);

    impl V {
        #[inline(always)]
        pub unsafe fn load(p: *const u8) -> V {
            V(_mm_loadu_si128(p as *const __m128i))
        }
        #[inline(always)]
        pub unsafe fn splat(b: u8) -> V {
            V(_mm_set1_epi8(b as i8))
        }
        #[inline(always)]
        pub unsafe fn eq(self, o: V) -> V {
            V(_mm_cmpeq_epi8(self.0, o.0))
        }
        #[inline(always)]
        pub unsafe fn mask(self) -> u64 {
            _mm_movemask_epi8(self.0) as u32 as u64
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod imp {
    use core::arch::aarch64::*;

    pub const STRIDE: usize = 4;
    pub const ALL: u64 = u64::MAX;

    #[derive(Clone, Copy)]
    pub struct V(uint8x16_t);

    impl V {
        #[inline(always)]
        pub unsafe fn load(p: *const u8) -> V {
            V(vld1q_u8(p))
        }
        #[inline(always)]
        pub unsafe fn splat(b: u8) -> V {
            V(vdupq_n_u8(b))
        }
        #[inline(always)]
        pub unsafe fn eq(self, o: V) -> V {
            V(vceqq_u8(self.0, o.0))
        }
        /// The usual NEON movemask substitute: narrowing shift right by 4
        /// keeps one nibble per byte lane.
        #[inline(always)]
        pub unsafe fn mask(self) -> u64 {
            let n = vshrn_n_u16(vreinterpretq_u16_u8(self.0), 4);
            vget_lane_u64(vreinterpret_u64_u8(n), 0)
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod imp {
    pub const STRIDE: usize = 1;
    pub const ALL: u64 = 0xffff;

    #[derive(Clone, Copy)]
    pub struct V([u8; 16]);

    impl V {
        #[inline(always)]
        pub unsafe fn load(p: *const u8) -> V {
            V(core::ptr::read_unaligned(p as *const [u8; 16]))
        }
        #[inline(always)]
        pub unsafe fn splat(b: u8) -> V {
            V([b; 16])
        }
        #[inline(always)]
        pub unsafe fn eq(self, o: V) -> V {
            let mut r = [0u8; 16];
            let mut i = 0;
            while i < 16 {
                r[i] = if self.0[i] == o.0[i] { 0xff } else { 0 };
                i += 1;
            }
            V(r)
        }
        #[inline(always)]
        pub unsafe fn mask(self) -> u64 {
            let mut m = 0u64;
            let mut i = 0;
            while i < 16 {
                m |= ((self.0[i] >> 7) as u64) << i;
                i += 1;
            }
            m
        }
    }
}

pub use imp::{ALL, STRIDE, V};
