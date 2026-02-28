# Ferrox

A low-latency order matching engine in Rust targeting **sub-50µs P99 latency** at **1M+ orders/second** with **zero heap allocation** on the hot path.

## Architecture

```text
                    ┌─────────────────────────────────────────────────┐
                    │                  Hot Path                       │
                    │                                                 │
  NIC/Client ──►  [Binary Decoder] ──► [Ring Buffer] ──► [Matching    │ ──► [UDP Multicast]
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

### Threading Model

Two threads, by design:

1. **Ingestion thread** — reads from network, decodes binary messages, writes to ring buffer.
2. **Matching thread** — reads from ring buffer, writes to WAL, executes matching, publishes results.

Single-threaded matching eliminates lock contention, cache invalidation from cross-core communication, and context switching overhead. This is the same architecture used by [LMAX Exchange](https://www.lmax.com/) to process 6M orders/second on a single thread.

## Design Decisions

| Decision | Choice | Rationale |
| -------- | ------ | --------- |
| Price representation | `i64` fixed-point ticks | No IEEE 754 rounding errors — unacceptable in financial systems |
| Order book | `HashMap<i64, VecDeque<Order>>` → intrusive linked list | O(1) insert/remove, no pointer chasing, cache-friendly |
| Memory management | Arena-based object pool (1M pre-allocated slots) | Zero `malloc`/`free` on hot path after startup |
| Cache alignment | `#[repr(align(64))]` on shared structs | Prevents false sharing between threads |
| Cross-thread comms | Lock-free SPSC ring buffer | `Acquire`/`Release` ordering — minimum barriers for correctness |
| Networking | UDP multicast + sequence numbers | One `sendto()` reaches all subscribers; gaps detected via seq_num |
| Persistence | WAL via `memmap2` + CRC32 per record | Deterministic replay for crash recovery |

## Performance

<!-- TODO: Replace with actual benchmark results from Phase 6 -->

| Metric | Target |
| ------ | ------ |
| P99 Latency | < 50µs |
| Throughput | 1,000,000+ orders/sec |
| Hot Path Allocations | 0 |

## Wire Protocol

All messages are fixed-size binary structs — no JSON, no Protobuf on the hot path.

| Message | Size | Description |
| ------- | ---- | ----------- |
| `NewOrder` | 32 bytes | Place a new limit order (side, price, quantity) |
| `CancelOrder` | 16 bytes | Cancel a resting order by ID |
| `ExecutionReport` | 48 bytes | Trade notification with monotonic sequence number |

## Crash Recovery

1. Load latest snapshot (if exists)
2. Open WAL, seek to first record after snapshot
3. Replay all records through the matching engine
4. Book state is now bit-exact to pre-crash state

The matching engine is fully deterministic: same input sequence always produces the same book state and execution reports.

## Development Phases

| Phase | Focus | Status |
| ----- | ----- | ------ |
| 1 | Domain model + matching logic (`HashMap` + `VecDeque`, correctness first) | Done |
| 2 | Performance data structures (arena, intrusive linked list, cache line padding) | Planned |
| 3 | Lock-free SPSC ring buffer (`AtomicUsize`, `Acquire`/`Release` ordering) | Planned |
| 4 | Binary protocol + UDP multicast (zero-copy codec, `socket2`) | Planned |
| 5 | Persistence + deterministic replay (`memmap2`, WAL, snapshots) | Planned |
| 6 | Benchmarking suite (`criterion`, `hdr_histogram`, P50/P90/P99/P99.9) | Planned |

## Build & Run

```bash
cargo build --release
cargo run --release
```

## Test

```bash
cargo test
```

## Benchmark

```bash
cargo bench
```

## Documentation

- [System Design](docs/SYSTEM_DESIGN.md) — Architecture, failure analysis, hardware considerations, deployment strategy
- [Development Phases](docs/PHASES.md) — Detailed deliverables, stack choices, and rationale for each phase

## License

MIT
