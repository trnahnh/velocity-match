use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use ferrox::matching::MatchingEngine;
use ferrox::order::{Order, Side};

fn make_order(id: u64, side: Side, price: i64, qty: u64) -> Order {
    Order::new(id, id, side, price, qty, id).unwrap()
}

fn engine(cap: u32) -> MatchingEngine {
    MatchingEngine::with_capacity(cap)
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");

    for &n in &[100, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::new("non_crossing", n), &n, |b, &n| {
            b.iter_batched(
                || engine(n as u32 + 16),
                |mut engine| {
                    for i in 0..n {
                        let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                        let price = if side == Side::Bid { 100 } else { 200 };
                        engine
                            .add_order(make_order(i + 1, side, price, 10))
                            .unwrap();
                    }
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.finish();
}

fn bench_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("match");

    group.bench_function("full_fill_1k", |b| {
        b.iter_batched(
            || {
                let mut e = engine(2_048);
                for i in 1..=1_000u64 {
                    e.add_order(make_order(i, Side::Ask, 100, 10)).unwrap();
                }
                e
            },
            |mut engine| {
                for i in 1..=1_000u64 {
                    engine
                        .add_order(make_order(1_000 + i, Side::Bid, 100, 10))
                        .unwrap();
                }
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("multi_level_sweep", |b| {
        b.iter_batched(
            || {
                let mut e = engine(2_048);
                for i in 0..100u64 {
                    for j in 0..10u64 {
                        let id = i * 10 + j + 1;
                        e.add_order(make_order(id, Side::Ask, 100 + i as i64, 10))
                            .unwrap();
                    }
                }
                e
            },
            |mut engine| {
                engine
                    .add_order(make_order(5_000, Side::Bid, 199, 5_000))
                    .unwrap();
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_cancel(c: &mut Criterion) {
    let mut group = c.benchmark_group("cancel");

    for &n in &[100, 1_000, 5_000] {
        group.bench_with_input(BenchmarkId::new("cancel_all", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut e = engine(n as u32 + 16);
                    for i in 1..=n {
                        e.add_order(make_order(i, Side::Bid, 100, 10)).unwrap();
                    }
                    e
                },
                |mut engine| {
                    for i in 1..=n {
                        engine.cancel_order(i).unwrap();
                    }
                },
                BatchSize::LargeInput,
            );
        });
    }

    for &n in &[100, 500, 1_000] {
        group.bench_with_input(BenchmarkId::new("cancel_best_level", n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let mut e = engine(n as u32 + 16);
                    for i in 0..n {
                        e.add_order(make_order(i + 1, Side::Bid, 1000 + i as i64, 10))
                            .unwrap();
                    }
                    e
                },
                |mut engine| {
                    for i in (0..n).rev() {
                        engine.cancel_order(i + 1).unwrap();
                    }
                },
                BatchSize::LargeInput,
            );
        });
    }

    group.bench_function("cancel_middle_of_1k", |b| {
        b.iter_batched(
            || {
                let mut e = engine(1_024);
                for i in 1..=1_000u64 {
                    e.add_order(make_order(i, Side::Bid, 100, 10)).unwrap();
                }
                e
            },
            |mut engine| {
                engine.cancel_order(500).unwrap();
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_mixed(c: &mut Criterion) {
    c.bench_function("mixed_workload_10k", |b| {
        b.iter_batched(
            || engine(16_384),
            |mut engine| {
                let mut next_id = 1u64;
                let mut resting_ids: Vec<u64> = Vec::new();

                for i in 0..10_000u64 {
                    let action = i % 10;
                    match action {
                        0..=5 => {
                            let side = if i % 2 == 0 { Side::Bid } else { Side::Ask };
                            let price = if side == Side::Bid { 100 } else { 200 };
                            let _ = engine.add_order(make_order(next_id, side, price, 10));
                            resting_ids.push(next_id);
                            next_id += 1;
                        }
                        6 | 7 => {
                            if let Some(id) = resting_ids.pop() {
                                let _ = engine.cancel_order(id);
                            }
                        }
                        _ => {
                            let _ = engine.add_order(make_order(next_id, Side::Bid, 200, 10));
                            next_id += 1;
                        }
                    }
                }
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    bench_insert,
    bench_match,
    bench_cancel,
    bench_mixed
);
criterion_main!(benches);
