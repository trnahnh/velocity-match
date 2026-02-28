# velocity-match

A low-latency order matching engine in Rust targeting **sub-50µs P99 latency** at **1M+ orders/second**.

## Architecture

```text
NIC/Client → [Binary Decoder] → [Ring Buffer (SPSC)] → [Matching Engine] → [UDP Multicast]
                                                              │
                                                         [Order Book]
                                                              │
                                                        [WAL / mmap]
```

- **Matching**: Price-time priority (FIFO) with O(1) insert, cancel, and top-of-book match
- **Memory**: Zero heap allocation on hot path — arena-based object pool (1M pre-allocated orders)
- **Concurrency**: Lock-free SPSC ring buffer (LMAX Disruptor pattern), single-threaded matching
- **Networking**: UDP multicast execution reports, zero-copy binary protocol
- **Persistence**: Write-ahead log via mmap, deterministic replay, periodic snapshots
- **Prices**: Fixed-point `i64` ticks — no floating-point on the trade path

## Performance

<!-- TODO: Replace with actual benchmark results from Phase 6 -->

| Metric | Target |
| ------ | ------ |
| P99 Latency | < 50µs |
| Throughput | 1,000,000+ orders/sec |
| Hot Path Allocations | 0 |

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

[System Design →](docs/SYSTEM_DESIGN.md) — Architecture, failure analysis, hardware considerations, deployment strategy.

## License

MIT
