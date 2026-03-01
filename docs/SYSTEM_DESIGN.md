# Ferrox — System Design Document

## 1. Overview

Ferrox is a low-latency order matching engine implementing price-time priority (FIFO) for a single-instrument limit order book. The engine targets sub-50µs P99 tick-to-trade latency at 1M+ orders/second throughput with zero heap allocation on the hot path.

This document covers the architecture, data flow, performance design decisions, failure modes, and deployment considerations.

---

## 2. Architecture

### 2.1 High-Level Data Flow

```text
                    ┌─────────────────────────────────────────────────┐
                    │                  Hot Path                       │
                    │                                                 │
  NIC/Client ──►  [Binary Decoder] ──► [Ring Buffer] ──► [Matching   │ ──► [UDP Multicast]
                    │                     (SPSC)          Engine]     │     ExecutionReports
                    │                                       │         │
                    │                                       ▼         │
                    │                                  [Order Book]   │
                    │                                   │         │   │
                    │                              Bid Side   Ask Side│
                    └─────────────────────────────────────────────────┘
                                                        │
                                                        ▼
                                                   [WAL / mmap]
                                                   Persistence
```

### 2.2 Component Responsibilities

| Component | Responsibility | Thread |
| --------- | -------------- | ------ |
| Binary Decoder | Deserialize inbound messages from raw bytes | Ingestion thread |
| Ring Buffer (SPSC) | Transfer decoded orders from ingestion to matching | Shared (lock-free) |
| Matching Engine | Execute price-time priority matching, manage order book | Matching thread |
| Order Book | Maintain bid/ask price levels with time-ordered queues | Matching thread |
| UDP Publisher | Broadcast execution reports via multicast | Matching thread |
| WAL Writer | Persist every inbound order before matching | Matching thread |

### 2.3 Threading Model

The system uses exactly **two threads**:

1. **Ingestion thread**: Reads from network, decodes binary messages, writes to ring buffer.
2. **Matching thread**: Reads from ring buffer, writes to WAL, executes matching, publishes results.

The matching engine is **single-threaded by design**. This eliminates lock contention, cache invalidation from cross-core communication, and context switching overhead. This is the same architecture used by LMAX Exchange to process 6M orders/second on a single thread.

---

## 3. Data Model

### 3.1 Order

```text
Order {
    id:        u64       // Unique order identifier
    side:      enum      // Bid | Ask
    price:     i64       // Fixed-point ticks (e.g., $150.05 = 15005 at tick_size=0.01)
    quantity:  u64       // Remaining quantity
    timestamp: u64       // Nanosecond timestamp for FIFO ordering
    prev:      u32       // Arena index of previous order in price level (intrusive list)
    next:      u32       // Arena index of next order in price level (intrusive list)
}
```

**Size**: Padded to 64 bytes (`#[repr(align(64))]`) to occupy exactly one cache line, preventing false sharing.

**Price representation**: All prices are `i64` integers in tick units. No floating-point arithmetic exists anywhere on the hot path. This eliminates IEEE 754 rounding errors that are unacceptable in financial systems.

### 3.2 Order Book

```text
OrderBook {
    bids:       BTreeMap<i64, PriceLevel>   // price → level (buy side, sorted)
    asks:       BTreeMap<i64, PriceLevel>   // price → level (sell side, sorted)
    best_bid:   Option<i64>                 // Cached best bid price
    best_ask:   Option<i64>                 // Cached best ask price
    pool:       Arena<Order>                // Pre-allocated order storage
}

PriceLevel {
    price:    i64
    qty:      u64       // Total quantity at this level
    count:    u32       // Number of orders
    head:     u32       // Arena index of first order (oldest, highest priority)
    tail:     u32       // Arena index of last order (newest)
}
```

### 3.3 Messages

All messages are fixed-size binary structs. No variable-length fields on the hot path.

```text
NewOrder {                          // 40 bytes, little-endian
    msg_type:   u8      // 0x01
    side:       u8      // 0=Bid, 1=Ask
    reserved:   [u8; 6]
    order_id:   u64
    trader_id:  u64     // Needed for self-trade prevention
    price:      i64
    quantity:   u64
}
// Timestamp assigned by ingestion thread, not on wire

CancelOrder {                       // 16 bytes
    msg_type:   u8      // 0x02
    reserved:   [u8; 7]
    order_id:   u64
}

ExecutionReport {                   // 48 bytes
    msg_type:       u8    // 0x03
    reserved:       [u8; 3]
    seq_num:        u32   // Monotonic sequence for gap detection
    taker_order_id: u64
    maker_order_id: u64
    price:          i64
    quantity:       u64
    timestamp:      u64
}
```

---

## 4. Core Algorithms

### 4.1 Price-Time Priority Matching

When a new Buy order arrives at price P with quantity Q:

```text
1. While Q > 0 AND best_ask <= P:
   a. level = asks[best_ask]
   b. order = level.head                    // Oldest order (FIFO)
   c. fill_qty = min(Q, order.quantity)
   d. Emit ExecutionReport(buy_id, order.id, best_ask, fill_qty)
   e. order.quantity -= fill_qty
   f. Q -= fill_qty
   g. If order.quantity == 0:
      - Remove order from level (O(1) linked list removal)
      - Return order to arena pool
      - If level is empty, remove level and update best_ask
2. If Q > 0:
   - Insert remaining as resting order on bid side at price P
```

Sell-side matching is symmetric.

**Complexity**: O(1) per fill against top-of-book. O(1) insertion of resting orders. O(1) cancellation via order ID → arena index lookup.

### 4.2 Best Price Tracking

Maintaining `best_bid` and `best_ask` avoids scanning all price levels on every match:

- On insert to empty level: compare with current best, update if better.
- On removal of last order at best level: O(log n) lookup via BTreeMap — `keys().next_back()` for best bid (max), `keys().next()` for best ask (min).

Price levels are stored in `BTreeMap<i64, PriceLevel>`, which keeps keys sorted. This replaces the earlier HashMap approach that required O(n) linear scan on level removal.

---

## 5. Memory Architecture

### 5.1 Object Pool (Arena)

```text
Arena<Order> {
    storage:    Vec<Order>      // Pre-allocated at startup (1M slots)
    free_head:  u32             // Head of free list (singly linked via `next` field)
    count:      u32             // Active orders
}
```

**Allocation**: `alloc()` pops from free list head — O(1), zero syscalls.
**Deallocation**: `dealloc(index)` pushes onto free list head — O(1), zero syscalls.

After startup, the hot path never calls `malloc`, `free`, `Box::new`, or any allocator. This eliminates allocator contention and GC pauses (Rust has no GC, but allocator fragmentation still matters).

### 5.2 Cache Line Optimization

```rust
#[repr(align(64))]
struct Order { /* 64 bytes */ }
```

Each `Order` occupies exactly one 64-byte cache line (x86_64). This prevents **false sharing**: when two threads access different `Order` objects that happen to share a cache line, the CPU would invalidate and reload the entire line on every write, destroying performance.

The ring buffer's head and tail indices are also padded to separate cache lines using a custom `CachePadded<AtomicUsize>` wrapper (`#[repr(align(64))]`, zero dependencies).

### 5.3 Memory Layout

```text
┌──────────────────────────────────────────────┐
│              Arena (pre-allocated)            │
│  [Order 0][Order 1][Order 2]...[Order 999999]│
│   64 bytes each, contiguous, cache-friendly   │
└──────────────────────────────────────────────┘

┌──────────────────────────────────────────────┐
│           Ring Buffer (pre-allocated)         │
│  [Slot 0][Slot 1][Slot 2]...[Slot 65535]     │
│   Power-of-2 capacity, bitmask indexing       │
└──────────────────────────────────────────────┘
```

---

## 6. Lock-Free Ring Buffer (SPSC Disruptor)

### 6.1 Design

```text
RingBuffer<T> {
    buffer:   Vec<MaybeUninit<T>>       // Fixed capacity, power-of-2
    capacity: usize                      // Always 2^n
    mask:     usize                      // capacity - 1 (replaces modulo)
    head:     CachePadded<AtomicUsize>   // Write position (ingestion thread)
    tail:     CachePadded<AtomicUsize>   // Read position (matching thread)
}
```

### 6.2 Operations

**Push (ingestion thread)**:

```text
1. current_head = head.load(Relaxed)
2. next = (current_head + 1) & mask
3. If next == tail.load(Acquire) → buffer full, apply back-pressure
4. Write data to buffer[current_head]
5. head.store(next, Release)            // Makes write visible to consumer
```

**Pop (matching thread)**:

```text
1. current_tail = tail.load(Relaxed)
2. If current_tail == head.load(Acquire) → buffer empty, spin
3. Read data from buffer[current_tail]
4. tail.store((current_tail + 1) & mask, Release)
```

### 6.3 Memory Ordering Rationale

- `Release` on store: guarantees all prior writes (the data in the slot) are visible before the index update.
- `Acquire` on load: guarantees the reading thread sees all writes made before the corresponding `Release`.
- `Relaxed` on local loads: no ordering needed when a thread reads its own index.

This is the minimum barrier strength needed for correctness. Stronger orderings (`SeqCst`) would add unnecessary fence instructions.

---

## 7. Networking

### 7.1 UDP Multicast

Execution reports are broadcast via UDP multicast so all subscribers receive trade data simultaneously without per-client TCP connections.

- Multicast group: configurable (e.g., `239.1.1.1:5001`)
- One `sendto()` call reaches all subscribers
- No connection state to manage

### 7.2 Gap Detection and Recovery

UDP is unreliable. Messages can be dropped, duplicated, or reordered.

Every outbound `ExecutionReport` carries a monotonically increasing `seq_num`. Subscribers track the last seen sequence number:

```text
If received.seq_num > expected:
    Log gap: missed messages [expected, received.seq_num - 1]
    Request retransmit via unicast TCP to recovery endpoint
If received.seq_num == expected:
    Process normally, increment expected
If received.seq_num < expected:
    Duplicate, discard
```

The recovery endpoint replays missed messages from the WAL.

---

## 8. Persistence and Crash Recovery

### 8.1 Write-Ahead Log (WAL)

Every inbound order is serialized to a memory-mapped file **before** the matching engine processes it. This guarantees durability — if the process crashes mid-match, the order is already on disk.

```text
WAL Record Format:
┌──────────┬──────────┬──────────────────┬──────────┐
│ Length   │ CRC32    │ Payload (binary) │ Padding  │
│ 4 bytes  │ 4 bytes  │ variable         │ to align │
└──────────┴──────────┴──────────────────┴──────────┘
```

- `memmap2` provides OS-managed page cache for write performance
- `crc32fast` detects corruption from partial writes
- Sequential append-only writes maximize disk throughput

### 8.2 Deterministic Replay

The matching engine is fully deterministic: given the same sequence of input orders, it produces the exact same book state and execution reports. No randomness, no system clock reads, no thread-ordering dependencies on the matching path.

Recovery procedure:

```text
1. Load latest snapshot (if exists)
2. Open WAL, seek to first record after snapshot
3. Replay all records through matching engine
4. Book state is now identical to pre-crash state
```

### 8.3 Snapshots

Every N orders (configurable, default 10,000), the engine serializes the full book state to a snapshot file using `bincode`. This bounds replay time — on recovery, only records after the last snapshot need replaying.

Snapshot contains: all resting orders, all price levels, best bid/ask, sequence number, arena state.

---

## 9. Failure Analysis

### 9.1 Ring Buffer Full (Back-Pressure)

**Cause**: Matching engine is slower than ingestion rate.

**Handling**: Ingestion thread spins on the full condition. If sustained for >N microseconds, log a warning. The ring buffer must be sized to absorb burst traffic (default: 65,536 slots). In production, the ingestion thread would apply back-pressure to upstream clients via TCP flow control.

**Impact**: Inbound orders are delayed, not dropped. No data loss.

### 9.2 Matching Engine Crash

**Cause**: Bug, hardware fault, OOM (should not happen with object pooling).

**Recovery**: Restart process → load snapshot → replay WAL → resume. Recovery time is bounded by snapshot frequency. With snapshots every 10,000 orders, worst case replays 10,000 orders (sub-second).

**Guarantee**: Deterministic replay ensures the recovered state is bit-exact.

### 9.3 UDP Subscriber Misses Messages

**Cause**: Network congestion, slow subscriber, NIC buffer overflow.

**Detection**: Subscriber detects gap via sequence numbers.

**Recovery**: Subscriber sends retransmit request to recovery service, which replays missed execution reports from the WAL.

### 9.4 WAL Corruption

**Cause**: Power loss mid-write, disk failure.

**Detection**: CRC32 checksum on each record. On replay, corrupted records are detected and the WAL is truncated to the last valid record.

**Impact**: At most one order lost (the one being written during the crash).

### 9.5 Arena Pool Exhaustion

**Cause**: More than 1M simultaneous resting orders.

**Handling**: Return an error to the ingestion layer. Do not dynamically resize — that would allocate on the hot path. Pool size should be configured based on expected market depth. Log pool utilization metrics for capacity planning.

---

## 10. Hardware Considerations

### 10.1 x86_64 vs ARM64

| Aspect | x86_64 | ARM64 (Apple Silicon) |
| ------ | ------ | --------------------- |
| Cache line size | 64 bytes | 128 bytes (Apple M-series) |
| Memory ordering | TSO (strong by default) | Weak (explicit barriers needed) |
| Atomics | LOCK prefix, relatively cheap | LL/SC or LSE, varies by impl |
| Padding target | `#[repr(align(64))]` | `#[repr(align(128))]` for Apple |

The code should use a compile-time constant for cache line size:

```rust
#[cfg(target_arch = "x86_64")]
const CACHE_LINE: usize = 64;

#[cfg(target_arch = "aarch64")]
const CACHE_LINE: usize = 128;
```

On ARM, the weaker memory model means the `Acquire`/`Release` orderings in the ring buffer actually emit barrier instructions, whereas on x86 they're often free due to TSO. Benchmark both.

### 10.2 NUMA Topology

On multi-socket servers, memory access to a remote NUMA node adds 50-100ns latency. The matching engine and its memory (arena, ring buffer, order book) must all reside on the same NUMA node.

In production: use `numactl --membind=0 --cpunodebind=0` to pin both thread and memory.

### 10.3 CPU Microarchitecture

- **Branch prediction**: The matching loop's hot path should be branch-free where possible. Use conditional moves (`cmov`) over branches for price comparisons.
- **Prefetching**: When walking the order queue, prefetch the next node while processing the current one.
- **TLB pressure**: The arena's contiguous allocation minimizes TLB misses compared to scattered heap allocations.

---

## 11. Deployment Strategy

### 11.1 CPU Pinning

```bash
# Isolate CPUs 2 and 3 from the kernel scheduler
# (set in GRUB: isolcpus=2,3)

# Pin ingestion thread to CPU 2
taskset -c 2 ./ferrox --thread ingestion

# Pin matching thread to CPU 3
taskset -c 3 ./ferrox --thread matching
```

Isolated CPUs are not used by any other process, eliminating scheduling jitter. The two threads should be on the same physical core's sibling hyperthreads or adjacent cores sharing L2 cache.

### 11.2 Kernel Tuning

```bash
# Disable transparent huge pages (causes latency spikes during compaction)
echo never > /sys/kernel/mm/transparent_hugepage/enabled

# Set CPU governor to performance (disable frequency scaling)
cpupower frequency-set -g performance

# Increase network socket buffer sizes
sysctl -w net.core.rmem_max=16777216
sysctl -w net.core.wmem_max=16777216

# Disable Nagle's algorithm (for TCP recovery channel)
# Done in code: socket.set_nodelay(true)
```

### 11.3 Monitoring

- **Latency**: P50/P90/P99/P99.9 via HdrHistogram, exported to Prometheus
- **Throughput**: Orders/second counter
- **Book depth**: Number of resting orders per side
- **Arena utilization**: Pool usage percentage (alert at 80%)
- **Ring buffer utilization**: High-water mark (alert at 75%)
- **WAL size**: Disk usage, trigger snapshot if growing too fast

---

## 12. Benchmarking Methodology

### 12.1 Latency Measurement

**Tick-to-trade**: Time from ring buffer read to execution report emission. Measured with `std::time::Instant` (uses `CLOCK_MONOTONIC` on Linux, `QueryPerformanceCounter` on Windows).

Record every measurement in `HdrHistogram` with microsecond precision. Report P50, P90, P99, P99.9 after 1M+ order warm-up to eliminate JIT/cache effects.

### 12.2 Load Testing

The load generator is a separate binary that:

1. Pre-generates 10M random orders (realistic price distribution around a midpoint)
2. Writes them into the ring buffer as fast as possible
3. Records the timestamp at write and the timestamp on the corresponding execution report
4. Produces a latency histogram and throughput measurement

### 12.3 Memory Validation

Under sustained 1M orders/sec load for 60 seconds:

- Heap allocation count must be zero (after startup)
- RSS must remain flat (no growth)
- Verified via DHAT or tracking allocator

---

## 13. Limitations and Future Work

**Current scope**: Single instrument, single matching engine instance.

**Not implemented (documented for interview discussion)**:

- Kernel bypass networking (DPDK/AF_XDP) — would eliminate kernel overhead on the network path
- Multiple instruments — would require a gateway/router distributing orders to per-instrument engines
- Aeron transport — production-grade reliable UDP multicast with built-in flow control
- FIX protocol gateway — standard protocol for order entry from external clients
- Hot-hot failover — secondary engine replaying the same WAL for zero-downtime recovery
