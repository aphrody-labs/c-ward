//! Differential tests against naive reference implementations, on random data
//! at every alignment, plus guard-page tests: a string that ends right before
//! a PROT_NONE page must be scanned without faulting.

use crate::qsort::{qsort, qsort_r};
use crate::string::*;
use core::ffi::{c_char, c_int, c_void};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self, alphabet: u8) -> u8 {
        b'a' + (self.next() % alphabet as u64) as u8
    }
}

fn sign(x: c_int) -> c_int {
    x.signum()
}

fn ref_cmp(a: &[u8], b: &[u8]) -> c_int {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return *x as c_int - *y as c_int;
        }
    }
    0
}

fn ref_strcmp(a: &[u8], b: &[u8], n: usize) -> c_int {
    let mut i = 0;
    while i < n {
        let (x, y) = (a[i], b[i]);
        if x != y || x == 0 {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
    0
}

#[test]
fn strlen_strnlen_all_offsets() {
    let mut buf = vec![b'x'; 256];
    for start in 0..48 {
        for l in 0..160 {
            buf.iter_mut().for_each(|b| *b = b'x');
            buf[start + l] = 0;
            let p = buf[start..].as_ptr() as *const c_char;
            unsafe {
                assert_eq!(strlen(p), l, "start {start} len {l}");
                for n in [0, 1, l / 2, l, l + 1, l + 40, usize::MAX] {
                    assert_eq!(strnlen(p, n), l.min(n), "start {start} len {l} n {n}");
                }
            }
        }
    }
}

#[test]
fn strchr_family() {
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for _ in 0..4000 {
        let start = (rng.next() % 32) as usize;
        let l = (rng.next() % 100) as usize;
        let mut buf = vec![0u8; start + l + 1 + 32];
        for b in &mut buf[start..start + l] {
            *b = rng.byte(6);
        }
        let s = &buf[start..start + l + 1];
        let p = s.as_ptr() as *const c_char;
        for c in [b'a', b'c', b'f', b'z', 0u8] {
            let first = s.iter().position(|&b| b == c);
            let last = s.iter().rposition(|&b| b == c);
            let nul_or = s.iter().position(|&b| b == c || b == 0).unwrap();
            unsafe {
                let r = strchr(p, c as c_int);
                assert_eq!(
                    if r.is_null() { None } else { Some(r as usize - p as usize) },
                    first
                );
                let r = strrchr(p, c as c_int);
                assert_eq!(
                    if r.is_null() { None } else { Some(r as usize - p as usize) },
                    last
                );
                assert_eq!(strchrnul(p, c as c_int) as usize - p as usize, nul_or);
                // c is converted to unsigned char: 0x100 + 'a' finds 'a'.
                let r = strchr(p, 0x100 + c as c_int);
                assert_eq!(
                    if r.is_null() { None } else { Some(r as usize - p as usize) },
                    first
                );
            }
        }
    }
}

#[test]
fn memchr_memrchr() {
    let mut rng = Rng(42);
    for _ in 0..4000 {
        let start = (rng.next() % 32) as usize;
        let n = (rng.next() % 140) as usize;
        let mut buf = vec![b'q'; start + n + 32];
        for b in &mut buf[start..start + n] {
            *b = rng.byte(8);
        }
        let s = &buf[start..start + n];
        let p = s.as_ptr() as *const c_void;
        for c in [b'a', b'h', b'q', b'z'] {
            unsafe {
                let r = memchr(p, c as c_int, n);
                assert_eq!(
                    if r.is_null() { None } else { Some(r as usize - p as usize) },
                    s.iter().position(|&b| b == c)
                );
                let r = memrchr(p, c as c_int, n);
                assert_eq!(
                    if r.is_null() { None } else { Some(r as usize - p as usize) },
                    s.iter().rposition(|&b| b == c)
                );
            }
        }
    }
    // An oversized length still stops at the first match.
    let s = b"hello\0";
    unsafe {
        let r = memchr(s.as_ptr() as *const c_void, 0, usize::MAX);
        assert_eq!(r as usize - s.as_ptr() as usize, 5);
    }
}

#[test]
fn memcmp_bcmp() {
    let mut rng = Rng(7);
    for _ in 0..6000 {
        let n = (rng.next() % 90) as usize;
        let (oa, ob) = ((rng.next() % 16) as usize, (rng.next() % 16) as usize);
        let mut a = vec![0u8; oa + n];
        let mut b = vec![0u8; ob + n];
        for i in 0..n {
            let v = (rng.next() & 0xff) as u8;
            a[oa + i] = v;
            b[ob + i] = v;
        }
        if n > 0 && rng.next() % 3 != 0 {
            let k = (rng.next() as usize) % n;
            b[ob + k] = b[ob + k].wrapping_add(1 + (rng.next() % 255) as u8);
        }
        let (sa, sb) = (&a[oa..], &b[ob..]);
        let expect = sign(ref_cmp(sa, sb));
        unsafe {
            let pa = sa.as_ptr() as *const c_void;
            let pb = sb.as_ptr() as *const c_void;
            assert_eq!(sign(memcmp(pa, pb, n)), expect, "n {n}");
            assert_eq!(bcmp(pa, pb, n) == 0, expect == 0);
        }
    }
    // Bytes compare as unsigned char.
    unsafe {
        let (a, b) = ([0x80u8], [0x01u8]);
        assert!(memcmp(a.as_ptr() as _, b.as_ptr() as _, 1) > 0);
    }
}

#[test]
fn strcmp_strncmp() {
    let mut rng = Rng(1234);
    for _ in 0..6000 {
        let la = (rng.next() % 70) as usize;
        let common = if la == 0 { 0 } else { (rng.next() as usize) % (la + 1) };
        let lb = common + (rng.next() % 20) as usize;
        let (oa, ob) = ((rng.next() % 16) as usize, (rng.next() % 16) as usize);
        let mut a = vec![0u8; oa + la + 1];
        let mut b = vec![0u8; ob + lb + 1];
        for i in 0..la {
            a[oa + i] = rng.byte(3);
        }
        for i in 0..lb {
            b[ob + i] = if i < common { a[oa + i] } else { rng.byte(3) };
        }
        let (sa, sb) = (&a[oa..], &b[ob..]);
        let full = la.max(lb) + 1;
        unsafe {
            let pa = sa.as_ptr() as *const c_char;
            let pb = sb.as_ptr() as *const c_char;
            let r = ref_strcmp(sa, sb, full);
            assert_eq!(sign(strcmp(pa, pb)), sign(r));
            for n in [0, 1, common, common + 1, full, full + 100] {
                let r = ref_strcmp(sa, sb, n.min(full));
                assert_eq!(sign(strncmp(pa, pb, n)), sign(r), "n {n}");
            }
        }
    }
}

#[test]
fn memmem_strstr() {
    let mut rng = Rng(99);
    for _ in 0..3000 {
        let hl = (rng.next() % 120) as usize;
        let nl = (rng.next() % 6) as usize;
        let mut h: Vec<u8> = (0..hl).map(|_| rng.byte(3)).collect();
        let mut n: Vec<u8> = (0..nl).map(|_| rng.byte(3)).collect();
        let expect = if nl == 0 {
            Some(0)
        } else {
            h.windows(nl).position(|w| w == &n[..])
        };
        unsafe {
            let r = memmem(h.as_ptr() as _, hl, n.as_ptr() as _, nl);
            assert_eq!(
                if r.is_null() { None } else { Some(r as usize - h.as_ptr() as usize) },
                expect
            );
            h.push(0);
            n.push(0);
            let r = strstr(h.as_ptr() as _, n.as_ptr() as _);
            assert_eq!(
                if r.is_null() { None } else { Some(r as usize - h.as_ptr() as usize) },
                expect
            );
        }
    }
}

/// Maps two pages, makes the second inaccessible and returns the first.
fn guarded_page() -> *mut u8 {
    unsafe {
        let p = libc::mmap(
            core::ptr::null_mut(),
            8192,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        );
        assert_ne!(p, libc::MAP_FAILED);
        assert_eq!(libc::mprotect((p as *mut u8).add(4096) as _, 4096, libc::PROT_NONE), 0);
        p as *mut u8
    }
}

#[test]
fn no_read_past_page_end() {
    let page = guarded_page();
    unsafe {
        for l in 0..40usize {
            for i in 0..4096 {
                *page.add(i) = b'k';
            }
            // String of length l whose terminator is the last byte of the page.
            let s = page.add(4096 - l - 1);
            *page.add(4095) = 0;
            let p = s as *const c_char;
            assert_eq!(strlen(p), l);
            assert_eq!(strnlen(p, usize::MAX), l);
            assert!(strchr(p, b'z' as c_int).is_null());
            assert_eq!(strchrnul(p, b'z' as c_int) as usize, page as usize + 4095);
            assert_eq!(strrchr(p, 0) as usize, page as usize + 4095);
            assert_eq!(memchr(s as _, 0, usize::MAX) as usize, page as usize + 4095);
            // Both strings end at the page end with different alignments.
            let q = page.add(4096 - (l / 2) - 1) as *const c_char;
            assert_eq!(sign(strcmp(p, q)), sign(ref_strcmp(
                core::slice::from_raw_parts(s, l + 1),
                core::slice::from_raw_parts(q as *const u8, l / 2 + 1),
                l / 2 + 1,
            )));
            assert_eq!(strcmp(p, p), 0);
            assert_eq!(strncmp(p, p, usize::MAX), 0);
            assert!(strstr(p, b"kz\0".as_ptr() as _).is_null());
            let r = memrchr(s as _, b'k' as c_int, l);
            assert_eq!(r.is_null(), l == 0);
        }
        libc::munmap(page as _, 8192);
    }
}

unsafe extern "C" fn cmp_u32(a: *const c_void, b: *const c_void) -> c_int {
    let (a, b) = (*(a as *const u32), *(b as *const u32));
    (a > b) as c_int - (a < b) as c_int
}

unsafe extern "C" fn cmp_rec_r(a: *const c_void, b: *const c_void, arg: *mut c_void) -> c_int {
    *(arg as *mut usize) += 1;
    let (a, b) = (*(a as *const u8), *(b as *const u8));
    a as c_int - b as c_int
}

/// Inconsistent comparator: must not crash or go out of bounds.
unsafe extern "C" fn cmp_random(_: *const c_void, _: *const c_void) -> c_int {
    static mut STATE: u64 = 0x1234_5678;
    STATE ^= STATE << 13;
    STATE ^= STATE >> 7;
    STATE ^= STATE << 17;
    (STATE % 3) as c_int - 1
}

#[test]
fn qsort_matches_sort() {
    let mut rng = Rng(5);
    for n in [0usize, 1, 2, 3, 15, 16, 17, 100, 129, 1000, 20000] {
        for pattern in 0..5 {
            let mut v: Vec<u32> = (0..n as u32)
                .map(|i| match pattern {
                    0 => rng.next() as u32,
                    1 => i,
                    2 => n as u32 - i,
                    3 => (rng.next() % 4) as u32,
                    _ => if i % 2 == 0 { i } else { n as u32 - i },
                })
                .collect();
            let mut expect = v.clone();
            expect.sort_unstable();
            unsafe { qsort(v.as_mut_ptr() as _, n, 4, Some(cmp_u32)) };
            assert_eq!(v, expect, "n {n} pattern {pattern}");
        }
    }
}

#[test]
fn qsort_r_odd_size_and_arg() {
    // 3-byte records (byte swaps), key in the first byte.
    let mut rng = Rng(77);
    let n = 5000;
    let mut v: Vec<[u8; 3]> = (0..n)
        .map(|_| {
            let k = (rng.next() & 0xff) as u8;
            [k, k ^ 0x5a, k.wrapping_mul(3)]
        })
        .collect();
    let mut calls = 0usize;
    unsafe {
        qsort_r(
            v.as_mut_ptr() as _,
            n,
            3,
            Some(cmp_rec_r),
            &mut calls as *mut usize as *mut c_void,
        )
    };
    assert!(v.windows(2).all(|w| w[0][0] <= w[1][0]));
    assert!(v.iter().all(|r| r[1] == r[0] ^ 0x5a && r[2] == r[0].wrapping_mul(3)));
    assert!(calls > 0);
}

#[test]
fn qsort_inconsistent_comparator_stays_in_bounds() {
    let mut v: Vec<u64> = (0..10000).collect();
    unsafe { qsort(v.as_mut_ptr() as _, v.len(), 8, Some(cmp_random)) };
    v.sort_unstable();
    assert_eq!(v, (0..10000).collect::<Vec<u64>>());
}
