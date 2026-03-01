use criterion::{Criterion, criterion_group, criterion_main};
use ferrox::matching::MatchingEngine;
use ferrox::order::{Order, Side};

use ferrox::protocol::{EngineCommand, NEW_ORDER_SIZE, encode_cancel_order, encode_new_order};

fn make_order(id: u64) -> Order {
    Order {
        id,
        trader_id: 1,
        side: if id % 2 == 0 { Side::Bid } else { Side::Ask },
        price: 10000 + (id % 100) as i64,
        quantity: 100,
        timestamp: id,
    }
}

fn bench_wal_encode_new_order(c: &mut Criterion) {
    let order = make_order(1);
    let mut buf = [0u8; NEW_ORDER_SIZE];

    c.bench_function("wal/encode_new_order", |b| {
        b.iter(|| {
            encode_new_order(&mut buf, &order).unwrap();
            crc32fast::hash(&buf[..NEW_ORDER_SIZE])
        })
    });
}

fn bench_wal_encode_crc_throughput(c: &mut Criterion) {
    let mut buf = [0u8; NEW_ORDER_SIZE];
    let orders: Vec<Order> = (1..=10_000).map(make_order).collect();

    c.bench_function("wal/encode+crc_10k", |b| {
        b.iter(|| {
            for order in &orders {
                encode_new_order(&mut buf, order).unwrap();
                crc32fast::hash(&buf[..NEW_ORDER_SIZE]);
            }
        })
    });
}

fn bench_snapshot_capture(c: &mut Criterion) {
    let mut engine = MatchingEngine::with_capacity(20_000);
    for i in 1..=10_000u64 {
        let order = Order::new(i, i, Side::Bid, 10000 - (i as i64 % 5000), 100, i).unwrap();
        engine.add_order(order).unwrap();
    }

    c.bench_function("snapshot/all_resting_orders_10k", |b| {
        b.iter(|| engine.book().all_resting_orders())
    });
}

fn bench_snapshot_serialize(c: &mut Criterion) {
    let mut engine = MatchingEngine::with_capacity(20_000);
    for i in 1..=10_000u64 {
        let order = Order::new(i, i, Side::Bid, 10000 - (i as i64 % 5000), 100, i).unwrap();
        engine.add_order(order).unwrap();
    }
    let orders = engine.book().all_resting_orders();

    c.bench_function("snapshot/bincode_serialize_10k", |b| {
        b.iter(|| bincode::serialize(&orders).unwrap())
    });
}

fn bench_snapshot_deserialize(c: &mut Criterion) {
    let mut engine = MatchingEngine::with_capacity(20_000);
    for i in 1..=10_000u64 {
        let order = Order::new(i, i, Side::Bid, 10000 - (i as i64 % 5000), 100, i).unwrap();
        engine.add_order(order).unwrap();
    }
    let orders = engine.book().all_resting_orders();
    let data = bincode::serialize(&orders).unwrap();

    c.bench_function("snapshot/bincode_deserialize_10k", |b| {
        b.iter(|| {
            let _: Vec<Order> = bincode::deserialize(&data).unwrap();
        })
    });
}

fn bench_restore_from_orders(c: &mut Criterion) {
    let mut engine = MatchingEngine::with_capacity(20_000);
    for i in 1..=10_000u64 {
        let order = Order::new(i, i, Side::Bid, 10000 - (i as i64 % 5000), 100, i).unwrap();
        engine.add_order(order).unwrap();
    }
    let orders = engine.book().all_resting_orders();

    c.bench_function("snapshot/restore_from_orders_10k", |b| {
        b.iter(|| {
            let mut new_engine = MatchingEngine::with_capacity(20_000);
            for o in &orders {
                new_engine.add_order(o.clone()).unwrap();
            }
        })
    });
}

fn bench_mixed_wal_encode(c: &mut Criterion) {
    let cmds: Vec<EngineCommand> = (1..=10_000u64)
        .map(|i| {
            if i % 5 == 0 {
                EngineCommand::CancelOrder { order_id: i - 1 }
            } else {
                EngineCommand::NewOrder(make_order(i))
            }
        })
        .collect();

    let mut buf = [0u8; NEW_ORDER_SIZE];

    c.bench_function("wal/mixed_encode+crc_10k", |b| {
        b.iter(|| {
            for cmd in &cmds {
                match cmd {
                    EngineCommand::NewOrder(order) => {
                        let n = encode_new_order(&mut buf, order).unwrap();
                        crc32fast::hash(&buf[..n]);
                    }
                    EngineCommand::CancelOrder { order_id } => {
                        let n = encode_cancel_order(&mut buf, *order_id).unwrap();
                        crc32fast::hash(&buf[..n]);
                    }
                }
            }
        })
    });
}

criterion_group!(
    benches,
    bench_wal_encode_new_order,
    bench_wal_encode_crc_throughput,
    bench_mixed_wal_encode,
    bench_snapshot_capture,
    bench_snapshot_serialize,
    bench_snapshot_deserialize,
    bench_restore_from_orders,
);
criterion_main!(benches);
