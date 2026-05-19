//! Lock-free single-producer / single-consumer byte ring.
//!
//! Used by every board's UART RX path: the interrupt handler pushes one
//! byte per FIFO entry, the main thread pops them on demand. Single-core
//! Cortex-M3, no SMP — Acquire/Release on the two indices is enough to
//! keep ISR and consumer from seeing each other's writes out of order.
//!
//! Sized at compile time; `N` must be a power of two so wrapping is a
//! mask rather than a modulo. The const assertion below catches violations
//! at compile time.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscRing<const N: usize> {
    buf: UnsafeCell<[u8; N]>,
    /// Next write index. Written by the producer only.
    head: AtomicUsize,
    /// Next read index. Written by the consumer only.
    tail: AtomicUsize,
}

// SAFETY: `push` is only ever called from one side (the ISR) and `pop`
// only from the other (main thread). The atomics on `head`/`tail`
// synchronize visibility of the buffer write/read across the IRQ
// boundary; the buffer itself is never aliased mutably from both sides.
unsafe impl<const N: usize> Sync for SpscRing<N> {}

impl<const N: usize> SpscRing<N> {
    pub const fn new() -> Self {
        // Forces a compile-time error if `N` isn't a power of two. The
        // inline `const { ... }` block is evaluated when `new` is
        // monomorphized for this particular `N`.
        const { assert!(N.is_power_of_two(), "SpscRing N must be a power of two") };
        Self {
            buf: UnsafeCell::new([0; N]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Push one byte. If the ring is full, the byte is dropped (the host
    /// will retransmit on the resulting CRC failure). Must only be called
    /// from the producer side.
    pub fn push(&self, byte: u8) {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next = (head + 1) & (N - 1);
        if next == tail {
            return;
        }
        unsafe { (*self.buf.get())[head] = byte };
        self.head.store(next, Ordering::Release);
    }

    /// Pop one byte. Returns `None` if the ring is empty. Must only be
    /// called from the consumer side.
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let byte = unsafe { (*self.buf.get())[tail] };
        self.tail.store((tail + 1) & (N - 1), Ordering::Release);
        Some(byte)
    }
}

impl<const N: usize> Default for SpscRing<N> {
    fn default() -> Self {
        Self::new()
    }
}
