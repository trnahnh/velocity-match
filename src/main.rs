use ferrox::matching::MatchingEngine;
use ferrox::order::{Order, Side};

fn main() {
    let mut engine = MatchingEngine::new();

    println!("Ferrox - Order Matching Engine");
    println!("ferrox v{}", env!("CARGO_PKG_VERSION"));
    println!("arena capacity: 1,048,576 slots (64 MB)");
    println!();

    let asks = [
        Order::new(1, 100, Side::Ask, 10_050, 50, 1).unwrap(),
        Order::new(2, 101, Side::Ask, 10_100, 30, 2).unwrap(),
        Order::new(3, 102, Side::Ask, 10_050, 20, 3).unwrap(),
    ];

    for order in asks {
        let id = order.id;
        let price = order.price;
        let qty = order.quantity;
        let result = engine.add_order(order).unwrap();
        println!(
            "ask id={} price={} qty={} -> fills={}",
            id,
            price,
            qty,
            result.fills.len()
        );
    }

    println!();

    let bid = Order::new(4, 200, Side::Bid, 10_050, 60, 4).unwrap();
    let id = bid.id;
    let price = bid.price;
    let qty = bid.quantity;
    let result = engine.add_order(bid).unwrap();
    println!(
        "bid id={} price={} qty={} -> fills={}",
        id,
        price,
        qty,
        result.fills.len()
    );

    for fill in &result.fills {
        println!(
            "  fill: maker={} taker={} price={} qty={}",
            fill.maker_order_id, fill.taker_order_id, fill.price, fill.quantity
        );
    }

    println!();
    println!(
        "book: best_bid={:?} best_ask={:?} orders={}",
        engine.book().best_bid(),
        engine.book().best_ask(),
        engine.book().order_count()
    );
}
