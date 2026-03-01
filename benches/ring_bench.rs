use criterion::{Criterion, criterion_group, criterion_main};
use ferrox::order::{Order, Side};
use ferrox::ring::ring_buffer;
use std::thread;

fn make_order(id: u64) -> Order {
    Order::new(
        id,
        id % 100,
        if id % 2 == 0 { Side::Bid } else { Side::Ask },
        10_000 + (id as i64 % 500),
        (id % 1000) + 1,
        id * 1000,
    )
    .unwrap()
}

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    let count = 1_000_000u64;

    group.bench_function("spsc_ring/1M", |b| {
        b.iter(|| {
            let (mut p, mut c) = ring_buffer::<u64>(8192);
            let producer = thread::spawn(move || {
                for i in 0..count {
                    while p.push(i).is_err() {
                        std::hint::spin_loop();
                    }
                }
            });
            for _ in 0..count {
                while c.pop().is_err() {
                    std::hint::spin_loop();
                }
            }
            producer.join().unwrap();
        });
    });

    group.bench_function("std_mpsc/1M", |b| {
        b.iter(|| {
            let (tx, rx) = std::sync::mpsc::channel::<u64>();
            let producer = thread::spawn(move || {
                for i in 0..count {
                    tx.send(i).unwrap();
                }
            });
            for _ in 0..count {
                rx.recv().unwrap();
            }
            producer.join().unwrap();
        });
    });

    group.bench_function("std_mpsc_spin/1M", |b| {
        b.iter(|| {
            let (tx, rx) = std::sync::mpsc::channel::<u64>();
            let producer = thread::spawn(move || {
                for i in 0..count {
                    tx.send(i).unwrap();
                }
            });
            for _ in 0..count {
                while rx.try_recv().is_err() {
                    std::hint::spin_loop();
                }
            }
            producer.join().unwrap();
        });
    });

    group.bench_function("spsc_ring_order/1M", |b| {
        b.iter(|| {
            let (mut p, mut c) = ring_buffer::<Order>(8192);
            let producer = thread::spawn(move || {
                for i in 0..count {
                    let order = make_order(i);
                    while p.push(order.clone()).is_err() {
                        std::hint::spin_loop();
                    }
                }
            });
            for _ in 0..count {
                while c.pop().is_err() {
                    std::hint::spin_loop();
                }
            }
            producer.join().unwrap();
        });
    });

    group.bench_function("std_mpsc_order/1M", |b| {
        b.iter(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Order>();
            let producer = thread::spawn(move || {
                for i in 0..count {
                    tx.send(make_order(i)).unwrap();
                }
            });
            for _ in 0..count {
                rx.recv().unwrap();
            }
            producer.join().unwrap();
        });
    });

    // Fair comparison: mpsc with try_recv spin loop (no blocking recv overhead)
    group.bench_function("std_mpsc_spin_order/1M", |b| {
        b.iter(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Order>();
            let producer = thread::spawn(move || {
                for i in 0..count {
                    tx.send(make_order(i)).unwrap();
                }
            });
            for _ in 0..count {
                while rx.try_recv().is_err() {
                    std::hint::spin_loop();
                }
            }
            producer.join().unwrap();
        });
    });

    group.finish();
}

fn bench_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency");

    group.bench_function("push_pop_alternating/1M", |b| {
        b.iter_custom(|iters| {
            let (mut p, mut c) = ring_buffer::<u64>(1024);
            let start = std::time::Instant::now();
            for i in 0..iters {
                p.push(i).unwrap();
                let _ = c.pop().unwrap();
            }
            start.elapsed()
        });
    });

    group.finish();
}

criterion_group!(benches, bench_throughput, bench_latency);
criterion_main!(benches);
