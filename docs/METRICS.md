# Performance Metrics

**Benchmark environment**: Windows 11, Rust 2024 edition, `cargo bench` (criterion 0.8.2, release profile).

---

## Phase 1 → Phase 2: Data Structure Upgrade

**What changed**: `HashMap<i64, VecDeque<Order>>` replaced with arena-based object pool + intrusive doubly linked lists. Cancel complexity: O(n) → O(1).

### Order Cancel

| Benchmark | Phase 1 | Phase 2 | Change |
| --- | --- | --- | --- |
| cancel_all/100 | 6.26 µs | 4.46 µs | -29% |
| cancel_all/1000 | 55.42 µs | 39.79 µs | -28% |
| cancel_all/5000 | 313.87 µs | 208.34 µs | -34% |
| cancel_middle_of_1k | 2.16 µs | 0.91 µs | -58% |

### Matching

| Benchmark | Phase 1 | Phase 2 | Change |
| --- | --- | --- | --- |
| match/full_fill_1k | 96.20 µs | 96.87 µs | ~0% |
| match/multi_level_sweep | 42.07 µs | 50.61 µs | +20% |

### Insert

| Benchmark | Phase 1 | Phase 2 | Change |
| --- | --- | --- | --- |
| insert/non_crossing/100 | 5.15 µs | 5.57 µs | ~0% |
| insert/non_crossing/1000 | 47.91 µs | 49.34 µs | ~0% |
| insert/non_crossing/10000 | 529.28 µs | 537.06 µs | ~0% |

### Mixed Workload

| Benchmark | Phase 1 | Phase 2 | Change |
| --- | --- | --- | --- |
| mixed_workload_10k | 1.19 ms | 0.69 ms | -42% |

### Memory

| Metric | Phase 1 | Phase 2 |
| --- | --- | --- |
| Order storage | Heap-allocated per order (`VecDeque<Order>`) | Pre-allocated arena (1M slots, 64 MB) |
| Order size | 48 bytes (unaligned) | 64 bytes (`#[repr(C, align(64))]`, 1 cache line) |
| Cancel lookup | O(1) index lookup + O(n) linear scan | O(1) index lookup + O(1) unlink |
| Hot-path `malloc` | Every insert/cancel | Zero (arena pre-allocated) |
| `unsafe` blocks | 0 | 0 |

### Test Suite

| Metric | Phase 1 | Phase 2 |
| --- | --- | --- |
| Test count | 37 | 53 |
| Test time | ~0.06s | ~0.13s |

Note: Phase 1 test time measured before arena was introduced. Phase 2 tests use `with_capacity(1024)` to keep arena small (64 KB vs 64 MB default).
