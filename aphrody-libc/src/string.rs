//! `<string.h>` leaf functions, 16 bytes per step.
//!
//! Unbounded scans (`strlen`, `strchr`, `strcmp`, …) only read whole aligned
//! 16-byte blocks or blocks proven not to cross a page, so they never fault
//! past the terminator. Bounded scans (`memchr`, `memrchr`, `strnlen`) also use
//! aligned blocks: callers that pass an oversized length (common with
//! `memchr(p, c, SIZE_MAX)`) still stop at the first match, as with musl.

use crate::simd::{crosses_page, first, lanes, last, ALL, V, W};
use core::ffi::{c_char, c_int, c_void};
use core::ptr::null_mut;

#[inline(always)]
fn align_down(p: *const u8) -> (*const u8, usize) {
    let off = p as usize & (W - 1);
    (p.wrapping_sub(off), off)
}

/// First `c` in `[s, s + n)`, scanning aligned blocks.
#[inline(always)]
pub(crate) unsafe fn find_byte(s: *const u8, c: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let v = V::splat(c);
    let (mut p, off) = align_down(s);
    let m = V::load(p).eq(v).mask() & lanes(off, W);
    if m != 0 {
        let i = first(m) - off;
        return if i < n { Some(i) } else { None };
    }
    let mut seen = W - off;
    loop {
        if n <= seen {
            return None;
        }
        p = p.add(W);
        let m = V::load(p).eq(v).mask();
        if m != 0 {
            let i = seen + first(m);
            return if i < n { Some(i) } else { None };
        }
        seen += W;
    }
}

/// Last `c` in `[s, s + n)`, scanning aligned blocks backwards.
#[inline(always)]
pub(crate) unsafe fn find_byte_rev(s: *const u8, c: u8, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let v = V::splat(c);
    let start = s as usize;
    let end = start + n;
    let mut block = (end - 1) & !(W - 1);
    loop {
        let lo = start.saturating_sub(block).min(W);
        let hi = (end - block).min(W);
        let m = V::load(block as *const u8).eq(v).mask() & lanes(lo, hi);
        if m != 0 {
            return Some(block + last(m) - start);
        }
        if block <= start {
            return None;
        }
        block -= W;
    }
}

#[inline(always)]
pub(crate) unsafe fn len(s: *const u8) -> usize {
    let z = V::splat(0);
    let (mut p, off) = align_down(s);
    let m = V::load(p).eq(z).mask() & lanes(off, W);
    if m != 0 {
        return first(m) - off;
    }
    loop {
        p = p.add(W);
        let m = V::load(p).eq(z).mask();
        if m != 0 {
            return p as usize - s as usize + first(m);
        }
    }
}

/// Pointer to the first `c` or to the terminator, whichever comes first.
#[inline(always)]
unsafe fn chr_or_nul(s: *const u8, c: u8) -> *const u8 {
    if c == 0 {
        return s.add(len(s));
    }
    let z = V::splat(0);
    let v = V::splat(c);
    let (mut p, off) = align_down(s);
    let b = V::load(p);
    let m = (b.eq(z).mask() | b.eq(v).mask()) & lanes(off, W);
    if m != 0 {
        return p.add(first(m));
    }
    loop {
        p = p.add(W);
        let b = V::load(p);
        let m = b.eq(z).mask() | b.eq(v).mask();
        if m != 0 {
            return p.add(first(m));
        }
    }
}

#[inline(always)]
pub(crate) unsafe fn cmp(a: *const u8, b: *const u8, n: usize) -> c_int {
    let mut i = 0;
    while i + W <= n {
        let m = !V::load(a.add(i)).eq(V::load(b.add(i))).mask() & ALL;
        if m != 0 {
            let k = i + first(m);
            return *a.add(k) as c_int - *b.add(k) as c_int;
        }
        i += W;
    }
    if i < n && n >= W {
        // Overlapping last block: the bytes before `i` already compared equal.
        let j = n - W;
        let m = !V::load(a.add(j)).eq(V::load(b.add(j))).mask() & ALL;
        if m != 0 {
            let k = j + first(m);
            return *a.add(k) as c_int - *b.add(k) as c_int;
        }
        return 0;
    }
    while i < n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
    0
}

/// `strncmp` with `n == usize::MAX` is `strcmp`.
#[inline(always)]
unsafe fn str_cmp(a: *const u8, b: *const u8, n: usize) -> c_int {
    let z = V::splat(0);
    let mut i = 0;
    while i < n {
        let (pa, pb) = (a.add(i), b.add(i));
        if n - i >= W && !crosses_page(pa) && !crosses_page(pb) {
            let va = V::load(pa);
            let m = (!va.eq(V::load(pb)).mask() & ALL) | va.eq(z).mask();
            if m != 0 {
                let k = first(m);
                return *pa.add(k) as c_int - *pb.add(k) as c_int;
            }
            i += W;
        } else {
            let (x, y) = (*pa, *pb);
            if x != y || x == 0 {
                return x as c_int - y as c_int;
            }
            i += 1;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    len(s as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn strnlen(s: *const c_char, n: usize) -> usize {
    find_byte(s as *const u8, 0, n).unwrap_or(n)
}

#[no_mangle]
pub unsafe extern "C" fn strchrnul(s: *const c_char, c: c_int) -> *mut c_char {
    chr_or_nul(s as *const u8, c as u8) as *mut c_char
}

#[no_mangle]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let p = chr_or_nul(s as *const u8, c as u8);
    if *p == c as u8 {
        p as *mut c_char
    } else {
        null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    let s = s as *const u8;
    match find_byte_rev(s, c as u8, len(s) + 1) {
        Some(i) => s.add(i) as *mut c_char,
        None => null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let s = s as *const u8;
    match find_byte(s, c as u8, n) {
        Some(i) => s.add(i) as *mut c_void,
        None => null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn memrchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let s = s as *const u8;
    match find_byte_rev(s, c as u8, n) {
        Some(i) => s.add(i) as *mut c_void,
        None => null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    cmp(a as *const u8, b as *const u8, n)
}

#[no_mangle]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    cmp(a as *const u8, b as *const u8, n)
}

#[no_mangle]
pub unsafe extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    str_cmp(a as *const u8, b as *const u8, usize::MAX)
}

#[no_mangle]
pub unsafe extern "C" fn strncmp(a: *const c_char, b: *const c_char, n: usize) -> c_int {
    str_cmp(a as *const u8, b as *const u8, n)
}

#[no_mangle]
pub unsafe extern "C" fn memmem(
    h: *const c_void,
    hl: usize,
    n: *const c_void,
    nl: usize,
) -> *mut c_void {
    if nl == 0 {
        return h as *mut c_void;
    }
    if nl > hl {
        return null_mut();
    }
    let hs = core::slice::from_raw_parts(h as *const u8, hl);
    let ns = core::slice::from_raw_parts(n as *const u8, nl);
    match memchr::memmem::find(hs, ns) {
        Some(i) => (h as *const u8).add(i) as *mut c_void,
        None => null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn strstr(h: *const c_char, n: *const c_char) -> *mut c_char {
    let (h, n) = (h as *const u8, n as *const u8);
    let nl = len(n);
    if nl == 0 {
        return h as *mut c_char;
    }
    if nl == 1 {
        let p = chr_or_nul(h, *n);
        return if *p == *n { p as *mut c_char } else { null_mut() };
    }
    let hl = len(h);
    memmem(h as *const c_void, hl, n as *const c_void, nl) as *mut c_char
}
