use std::cell::UnsafeCell;
use std::fmt;
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[repr(align(64))]
pub struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// Returned when pushing to a full ring buffer. Contains the rejected value.
pub struct Full<T>(pub T);

impl<T> fmt::Debug for Full<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Full(..)")
    }
}

/// Returned when popping from an empty ring buffer.
#[derive(Debug)]
pub struct Empty;

struct RingBufferInner<T> {
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    capacity: usize,
    mask: usize,
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
}

// SAFETY: The SPSC protocol guarantees that only the Producer writes to slots
// and advances `head`, while only the Consumer reads from slots and advances
// `tail`. The Acquire/Release ordering on the atomic cursors establishes the
// necessary happens-before relationships. `UnsafeCell` access is safe because
// each slot is exclusively accessed by one side at a time.
unsafe impl<T: Send> Sync for RingBufferInner<T> {}

impl<T> Drop for RingBufferInner<T> {
    fn drop(&mut self) {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);

        // SAFETY: We have exclusive `&mut self` access (Drop guarantees no
        // concurrent readers). Items in `tail..head` were written by the
        // producer and never read by the consumer, so they are initialized
        // and need dropping.
        let mut i = tail;
        while i != head {
            unsafe {
                let slot = &mut *self.buffer[i & self.mask].get();
                slot.assume_init_drop();
            }
            i = i.wrapping_add(1);
        }
    }
}

pub struct Producer<T> {
    inner: Arc<RingBufferInner<T>>,
    cached_head: usize,
    cached_tail: usize,
}

// SAFETY: Producer is the sole writer. It can be sent to another thread
// as long as T: Send (values cross thread boundaries).
unsafe impl<T: Send> Send for Producer<T> {}

impl<T> Producer<T> {
    /// Try to push a value into the ring buffer.
    ///
    /// Returns `Err(Full(value))` if the buffer is full.
    pub fn push(&mut self, value: T) -> Result<(), Full<T>> {
        let head = self.cached_head;

        if head.wrapping_sub(self.cached_tail) == self.inner.capacity {
            self.cached_tail = self.inner.tail.load(Ordering::Acquire);
            if head.wrapping_sub(self.cached_tail) == self.inner.capacity {
                return Err(Full(value));
            }
        }

        // SAFETY: Producer has exclusive write access to buffer[head & mask].
        // The slot has been released by the consumer (tail has advanced past it)
        // or was never written (initial state). The Acquire load of `tail`
        // above ensures the consumer's read of the previous value is complete.
        unsafe {
            (*self.inner.buffer[head & self.inner.mask].get()).write(value);
        }

        self.inner.head.store(head.wrapping_add(1), Ordering::Release);
        self.cached_head = head.wrapping_add(1);

        Ok(())
    }

    /// Returns the capacity of the ring buffer.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

pub struct Consumer<T> {
    inner: Arc<RingBufferInner<T>>,
    cached_tail: usize,
    cached_head: usize,
}

// SAFETY: Consumer is the sole reader. It can be sent to another thread
// as long as T: Send (values cross thread boundaries).
unsafe impl<T: Send> Send for Consumer<T> {}

impl<T> Consumer<T> {
    /// Try to pop a value from the ring buffer.
    ///
    /// Returns `Err(Empty)` if the buffer is empty.
    pub fn pop(&mut self) -> Result<T, Empty> {
        let tail = self.cached_tail;

        if tail == self.cached_head {
            self.cached_head = self.inner.head.load(Ordering::Acquire);
            if tail == self.cached_head {
                return Err(Empty);
            }
        }

        // SAFETY: Consumer has exclusive read access to buffer[tail & mask].
        // The slot was written by the producer (head has advanced past it).
        // The Acquire load of `head` above ensures the producer's write is
        // visible.
        let value = unsafe {
            (*self.inner.buffer[tail & self.inner.mask].get()).assume_init_read()
        };

        self.inner.tail.store(tail.wrapping_add(1), Ordering::Release);
        self.cached_tail = tail.wrapping_add(1);

        Ok(value)
    }

    /// Returns the capacity of the ring buffer.
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }
}

/// Creates a new SPSC ring buffer with the given capacity.
///
/// Capacity must be a power of 2 and greater than zero.
///
/// # Panics
///
/// Panics if `capacity` is zero or not a power of two.
pub fn ring_buffer<T: Send>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    assert!(capacity > 0, "ring buffer capacity must be greater than zero");
    assert!(
        capacity.is_power_of_two(),
        "ring buffer capacity must be a power of two, got {capacity}"
    );

    let mut buffer = Vec::with_capacity(capacity);
    for _ in 0..capacity {
        buffer.push(UnsafeCell::new(MaybeUninit::uninit()));
    }

    let inner = Arc::new(RingBufferInner {
        buffer: buffer.into_boxed_slice(),
        capacity,
        mask: capacity - 1,
        head: CachePadded::new(AtomicUsize::new(0)),
        tail: CachePadded::new(AtomicUsize::new(0)),
    });

    let producer = Producer {
        inner: Arc::clone(&inner),
        cached_head: 0,
        cached_tail: 0,
    };

    let consumer = Consumer {
        inner,
        cached_tail: 0,
        cached_head: 0,
    };

    (producer, consumer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize as StdAtomicUsize;
    use std::thread;

    #[test]
    fn push_pop_single() {
        let (mut p, mut c) = ring_buffer::<u64>(4);
        p.push(42).unwrap();
        assert_eq!(c.pop().unwrap(), 42);
    }

    #[test]
    fn push_pop_fifo() {
        let (mut p, mut c) = ring_buffer::<u64>(8);
        for i in 0..8 {
            p.push(i).unwrap();
        }
        for i in 0..8 {
            assert_eq!(c.pop().unwrap(), i);
        }
    }

    #[test]
    fn full_returns_error() {
        let (mut p, mut _c) = ring_buffer::<u64>(4);
        for i in 0..4 {
            p.push(i).unwrap();
        }
        let err = p.push(99).unwrap_err();
        assert_eq!(err.0, 99);
    }

    #[test]
    fn empty_returns_error() {
        let (_p, mut c) = ring_buffer::<u64>(4);
        assert!(c.pop().is_err());
    }

    #[test]
    fn wraparound() {
        let (mut p, mut c) = ring_buffer::<u64>(4);
        for i in 0..100 {
            p.push(i).unwrap();
            assert_eq!(c.pop().unwrap(), i);
        }
    }

    #[test]
    fn fill_then_drain() {
        let (mut p, mut c) = ring_buffer::<u64>(8);

        for i in 0..8 {
            p.push(i).unwrap();
        }
        assert!(p.push(99).is_err());

        for i in 0..8 {
            assert_eq!(c.pop().unwrap(), i);
        }
        assert!(c.pop().is_err());

        for i in 100..108 {
            p.push(i).unwrap();
        }
        for i in 100..108 {
            assert_eq!(c.pop().unwrap(), i);
        }
    }

    #[test]
    fn capacity_accessor() {
        let (p, c) = ring_buffer::<u64>(16);
        assert_eq!(p.capacity(), 16);
        assert_eq!(c.capacity(), 16);
    }

    #[test]
    #[should_panic(expected = "greater than zero")]
    fn zero_capacity_panics() {
        ring_buffer::<u64>(0);
    }

    #[test]
    #[should_panic(expected = "power of two")]
    fn non_power_of_two_panics() {
        ring_buffer::<u64>(3);
    }

    #[test]
    fn drop_remaining_items() {
        let drop_count = Arc::new(StdAtomicUsize::new(0));

        struct DropCounter(Arc<StdAtomicUsize>);
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let (mut p, _c) = ring_buffer::<DropCounter>(4);
            p.push(DropCounter(Arc::clone(&drop_count))).unwrap();
            p.push(DropCounter(Arc::clone(&drop_count))).unwrap();
            p.push(DropCounter(Arc::clone(&drop_count))).unwrap();
        }

        assert_eq!(drop_count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn concurrent_push_pop() {
        let (mut p, mut c) = ring_buffer::<u64>(1024);
        let count = 100_000u64;

        let producer = thread::spawn(move || {
            for i in 0..count {
                while p.push(i).is_err() {
                    thread::yield_now();
                }
            }
        });

        let mut received = Vec::with_capacity(count as usize);
        for _ in 0..count {
            loop {
                match c.pop() {
                    Ok(v) => {
                        received.push(v);
                        break;
                    }
                    Err(_) => thread::yield_now(),
                }
            }
        }

        producer.join().unwrap();

        let expected: Vec<u64> = (0..count).collect();
        assert_eq!(received, expected);
    }

    #[test]
    fn concurrent_backpressure() {
        let (mut p, mut c) = ring_buffer::<u64>(16);
        let count = 100_000u64;

        let producer = thread::spawn(move || {
            for i in 0..count {
                while p.push(i).is_err() {
                    thread::yield_now();
                }
            }
        });

        let mut received = Vec::with_capacity(count as usize);
        for _ in 0..count {
            loop {
                match c.pop() {
                    Ok(v) => {
                        received.push(v);
                        break;
                    }
                    Err(_) => thread::yield_now(),
                }
            }
        }

        producer.join().unwrap();

        let expected: Vec<u64> = (0..count).collect();
        assert_eq!(received, expected);
    }

    #[test]
    fn concurrent_order_struct() {
        use crate::order::{Order, Side};

        let (mut p, mut c) = ring_buffer::<Order>(256);
        let count = 10_000u64;

        let producer = thread::spawn(move || {
            for i in 0..count {
                let order = Order::new(
                    i,
                    i % 100,
                    if i % 2 == 0 { Side::Bid } else { Side::Ask },
                    10_000 + (i as i64 % 500),
                    (i % 1000) + 1,
                    i * 1000,
                )
                .unwrap();
                while p.push(order.clone()).is_err() {
                    thread::yield_now();
                }
            }
        });

        let mut received = Vec::with_capacity(count as usize);
        for _ in 0..count {
            loop {
                match c.pop() {
                    Ok(order) => {
                        received.push(order);
                        break;
                    }
                    Err(_) => thread::yield_now(),
                }
            }
        }

        producer.join().unwrap();

        for (i, order) in received.iter().enumerate() {
            let i = i as u64;
            assert_eq!(order.id, i);
            assert_eq!(order.trader_id, i % 100);
            assert_eq!(
                order.side,
                if i % 2 == 0 { Side::Bid } else { Side::Ask }
            );
            assert_eq!(order.price, 10_000 + (i as i64 % 500));
            assert_eq!(order.quantity, (i % 1000) + 1);
            assert_eq!(order.timestamp, i * 1000);
        }
    }
}
