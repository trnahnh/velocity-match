#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub id: u64,
    pub trader_id: u64,
    pub side: Side,
    pub price: i64,
    pub quantity: u64,
    pub timestamp: u64,
}

impl Order {
    pub fn new(
        id: u64,
        trader_id: u64,
        side: Side,
        price: i64,
        quantity: u64,
        timestamp: u64,
    ) -> Option<Self> {
        if quantity == 0 {
            return None;
        }
        Some(Self {
            id,
            trader_id,
            side,
            price,
            quantity,
            timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_bid_order() {
        let order = Order::new(1, 1, Side::Bid, 15005, 100, 1_000_000).unwrap();
        assert_eq!(order.id, 1);
        assert_eq!(order.trader_id, 1);
        assert_eq!(order.side, Side::Bid);
        assert_eq!(order.price, 15005);
        assert_eq!(order.quantity, 100);
        assert_eq!(order.timestamp, 1_000_000);
    }

    #[test]
    fn create_ask_order() {
        let order = Order::new(2, 1, Side::Ask, 15010, 50, 2_000_000).unwrap();
        assert_eq!(order.side, Side::Ask);
        assert_eq!(order.quantity, 50);
    }

    #[test]
    fn reject_zero_quantity() {
        assert!(Order::new(1, 1, Side::Bid, 15005, 0, 1_000_000).is_none());
    }

    #[test]
    fn negative_price_allowed() {
        let order = Order::new(1, 1, Side::Bid, -100, 10, 0);
        assert!(order.is_some());
    }
}
