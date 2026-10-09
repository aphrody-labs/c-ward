//! `qsort`/`qsort_r` (glibc/musl 1.2.3+ signature): introsort without
//! allocation. Median-of-three (ninther above 128 elements) quicksort with a
//! Sedgewick partition, insertion sort below 16 elements, heapsort once the
//! recursion depth exceeds 2·log2(n). Every index is bounds-checked against
//! the range, so an inconsistent comparator yields an unspecified order but
//! never an out-of-bounds access (musl's smoothsort gives the same guarantee).
//!
//! musl's smoothsort is O(n) on sorted input but 1.5–3× slower than introsort
//! on random input because every sift step is an indirect call plus a
//! byte-wise swap; here swaps go by 8-byte words when size and alignment
//! allow.

use core::ffi::{c_int, c_void};

type Cmp = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;
type CmpR = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> c_int;

const INSERTION: usize = 16;
const NINTHER: usize = 128;

struct Sorter<F: FnMut(*const u8, *const u8) -> c_int> {
    base: *mut u8,
    size: usize,
    words: bool,
    cmp: F,
}

impl<F: FnMut(*const u8, *const u8) -> c_int> Sorter<F> {
    #[inline(always)]
    fn at(&self, i: usize) -> *mut u8 {
        // SAFETY: callers only pass i < nmemb.
        unsafe { self.base.add(i * self.size) }
    }

    #[inline(always)]
    fn less(&mut self, i: usize, j: usize) -> bool {
        let (a, b) = (self.at(i), self.at(j));
        (self.cmp)(a as *const u8, b as *const u8) < 0
    }

    #[inline(always)]
    fn swap(&mut self, i: usize, j: usize) {
        if i == j {
            return;
        }
        let (a, b) = (self.at(i), self.at(j));
        // SAFETY: distinct elements of `size` bytes inside the array.
        unsafe {
            if self.words {
                let (a, b) = (a as *mut u64, b as *mut u64);
                let mut k = 0;
                while k < self.size / 8 {
                    let t = *a.add(k);
                    *a.add(k) = *b.add(k);
                    *b.add(k) = t;
                    k += 1;
                }
            } else {
                let mut k = 0;
                while k < self.size {
                    let t = *a.add(k);
                    *a.add(k) = *b.add(k);
                    *b.add(k) = t;
                    k += 1;
                }
            }
        }
    }

    fn insertion(&mut self, lo: usize, hi: usize) {
        let mut i = lo + 1;
        while i < hi {
            let mut j = i;
            while j > lo && self.less(j, j - 1) {
                self.swap(j, j - 1);
                j -= 1;
            }
            i += 1;
        }
    }

    fn sift_down(&mut self, lo: usize, mut root: usize, n: usize) {
        loop {
            let mut child = 2 * root + 1;
            if child >= n {
                return;
            }
            if child + 1 < n && self.less(lo + child, lo + child + 1) {
                child += 1;
            }
            if !self.less(lo + root, lo + child) {
                return;
            }
            self.swap(lo + root, lo + child);
            root = child;
        }
    }

    fn heapsort(&mut self, lo: usize, hi: usize) {
        let n = hi - lo;
        let mut i = n / 2;
        while i > 0 {
            i -= 1;
            self.sift_down(lo, i, n);
        }
        let mut end = n;
        while end > 1 {
            end -= 1;
            self.swap(lo, lo + end);
            self.sift_down(lo, 0, end);
        }
    }

    /// Index of the median of three elements.
    fn median3(&mut self, a: usize, b: usize, c: usize) -> usize {
        if self.less(a, b) {
            if self.less(b, c) {
                b
            } else if self.less(a, c) {
                c
            } else {
                a
            }
        } else if self.less(a, c) {
            a
        } else if self.less(b, c) {
            c
        } else {
            b
        }
    }

    fn sort(&mut self, mut lo: usize, mut hi: usize, mut depth: u32) {
        loop {
            let n = hi - lo;
            if n <= INSERTION {
                self.insertion(lo, hi);
                return;
            }
            if depth == 0 {
                self.heapsort(lo, hi);
                return;
            }
            depth -= 1;

            let mid = lo + n / 2;
            let pivot = if n > NINTHER {
                let s = n / 8;
                let a = self.median3(lo, lo + s, lo + 2 * s);
                let b = self.median3(mid - s, mid, mid + s);
                let c = self.median3(hi - 1 - 2 * s, hi - 1 - s, hi - 1);
                self.median3(a, b, c)
            } else {
                self.median3(lo, mid, hi - 1)
            };
            self.swap(lo, pivot);

            // Pivot at `lo`. Equal keys stop both scans, which keeps runs of
            // duplicates balanced.
            let mut i = lo;
            let mut j = hi;
            loop {
                loop {
                    i += 1;
                    if i >= hi || !self.less(i, lo) {
                        break;
                    }
                }
                loop {
                    j -= 1;
                    if j <= lo || !self.less(lo, j) {
                        break;
                    }
                }
                if i >= j {
                    break;
                }
                self.swap(i, j);
            }
            self.swap(lo, j);

            // Recurse into the smaller side, loop on the larger: O(log n) stack.
            if j - lo < hi - (j + 1) {
                self.sort(lo, j, depth);
                lo = j + 1;
            } else {
                self.sort(j + 1, hi, depth);
                hi = j;
            }
        }
    }
}

unsafe fn sort_with<F: FnMut(*const u8, *const u8) -> c_int>(
    base: *mut c_void,
    nmemb: usize,
    size: usize,
    cmp: F,
) {
    if nmemb < 2 || size == 0 {
        return;
    }
    let base = base as *mut u8;
    let words = size % 8 == 0 && base as usize % 8 == 0;
    let mut s = Sorter {
        base,
        size,
        words,
        cmp,
    };
    let depth = 2 * (usize::BITS - nmemb.leading_zeros());
    s.sort(0, nmemb, depth);
}

#[no_mangle]
pub unsafe extern "C" fn qsort(base: *mut c_void, nmemb: usize, size: usize, compar: Option<Cmp>) {
    let Some(compar) = compar else { return };
    sort_with(base, nmemb, size, |a, b| {
        compar(a as *const c_void, b as *const c_void)
    });
}

#[no_mangle]
pub unsafe extern "C" fn qsort_r(
    base: *mut c_void,
    nmemb: usize,
    size: usize,
    compar: Option<CmpR>,
    arg: *mut c_void,
) {
    let Some(compar) = compar else { return };
    sort_with(base, nmemb, size, |a, b| {
        compar(a as *const c_void, b as *const c_void, arg)
    });
}
