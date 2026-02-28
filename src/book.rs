use std::collections::{HashMap, VecDeque};

use crate::order::{Order, Side};

#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderLocation {
    pub(crate) side: Side,
    pub(crate) price: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    DuplicateOrderId(u64),
    OrderNotFound(u64),
    PriceLevelNotFound(i64),
    FillExceedsQuantity { available: u64, requested: u64 },
}
#[derive(Debug)]
pub struct OrderBook {
    bids: HashMap<i64, VecDeque<Order>>,
    asks: HashMap<i64, VecDeque<Order>>,
    best_bid: Option<i64>,
    best_ask: Option<i64>,
    order_index: HashMap<u64, OrderLocation>,
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: HashMap::new(),
            asks: HashMap::new(),
            best_bid: None,
            best_ask: None,
            order_index: HashMap::new(),
        }
    }

    pub fn best_bid(&self) -> Option<i64> {
        self.best_bid
    }

    pub fn best_ask(&self) -> Option<i64> {
        self.best_ask
    }

    pub fn order_count(&self) -> usize {
        self.order_index.len()
    }

    pub(crate) fn insert_order(&mut self, order: Order) -> Result<(), BookError> {
        if self.order_index.contains_key(&order.id) {
            return Err(BookError::DuplicateOrderId(order.id));
        }

        let location = OrderLocation {
            side: order.side,
            price: order.price,
        };

        let levels = match order.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        levels
            .entry(order.price)
            .or_default()
            .push_back(order.clone());

        self.order_index.insert(order.id, location);
        self.update_best_after_insert(order.side, order.price);

        Ok(())
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<Order, BookError> {
        let location = self
            .order_index
            .remove(&order_id)
            .ok_or(BookError::OrderNotFound(order_id))?;

        let levels = match location.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let level = levels
            .get_mut(&location.price)
            .ok_or(BookError::PriceLevelNotFound(location.price))?;

        let pos = level
            .iter()
            .position(|o| o.id == order_id)
            .ok_or(BookError::OrderNotFound(order_id))?;

        let order = level.remove(pos).expect("position was just found");

        if level.is_empty() {
            levels.remove(&location.price);
            self.recompute_best(location.side);
        }

        Ok(order)
    }

    pub(crate) fn peek_front(&self, side: Side, price: i64) -> Option<&Order> {
        match side {
            Side::Bid => self.bids.get(&price)?.front(),
            Side::Ask => self.asks.get(&price)?.front(),
        }
    }

    pub(crate) fn reduce_front_quantity(
        &mut self,
        side: Side,
        price: i64,
        fill_qty: u64,
    ) -> Result<u64, BookError> {
        // Collect removal info before touching order_index to avoid overlapping borrows.
        let (remaining, removed_id, level_empty) = {
            let levels = match side {
                Side::Bid => &mut self.bids,
                Side::Ask => &mut self.asks,
            };
            let level = levels
                .get_mut(&price)
                .ok_or(BookError::PriceLevelNotFound(price))?;

            let front = level
                .front_mut()
                .ok_or(BookError::PriceLevelNotFound(price))?;

            if fill_qty > front.quantity {
                return Err(BookError::FillExceedsQuantity {
                    available: front.quantity,
                    requested: fill_qty,
                });
            }

            front.quantity -= fill_qty;
            let remaining = front.quantity;

            if remaining == 0 {
                let removed = level.pop_front().expect("front was just accessed");
                let empty = level.is_empty();
                (0, Some(removed.id), empty)
            } else {
                (remaining, None, false)
            }
        };

        if let Some(id) = removed_id {
            self.order_index.remove(&id);

            if level_empty {
                let levels = match side {
                    Side::Bid => &mut self.bids,
                    Side::Ask => &mut self.asks,
                };
                levels.remove(&price);
                self.recompute_best(side);
            }
        }

        Ok(remaining)
    }

    fn update_best_after_insert(&mut self, side: Side, price: i64) {
        match side {
            Side::Bid => {
                self.best_bid = Some(self.best_bid.map_or(price, |b| b.max(price)));
            }
            Side::Ask => {
                self.best_ask = Some(self.best_ask.map_or(price, |a| a.min(price)));
            }
        }
    }

    fn recompute_best(&mut self, side: Side) {
        match side {
            Side::Bid => {
                self.best_bid = self.bids.keys().copied().max();
            }
            Side::Ask => {
                self.best_ask = self.asks.keys().copied().min();
            }
        }
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::Order;

    fn bid(id: u64, price: i64, qty: u64, ts: u64) -> Order {
        Order::new(id, Side::Bid, price, qty, ts).unwrap()
    }

    fn ask(id: u64, price: i64, qty: u64, ts: u64) -> Order {
        Order::new(id, Side::Ask, price, qty, ts).unwrap()
    }

    #[test]
    fn insert_and_best_prices() {
        let mut book = OrderBook::new();
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 102, 10, 2)).unwrap();
        book.insert_order(ask(3, 105, 10, 3)).unwrap();
        book.insert_order(ask(4, 103, 10, 4)).unwrap();

        assert_eq!(book.best_bid(), Some(102));
        assert_eq!(book.best_ask(), Some(103));
        assert_eq!(book.order_count(), 4);
    }

    #[test]
    fn duplicate_id_rejected() {
        let mut book = OrderBook::new();
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        let err = book.insert_order(ask(1, 105, 5, 2)).unwrap_err();
        assert_eq!(err, BookError::DuplicateOrderId(1));
    }

    #[test]
    fn cancel_order_updates_best() {
        let mut book = OrderBook::new();
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 102, 10, 2)).unwrap();

        let cancelled = book.cancel_order(2).unwrap();
        assert_eq!(cancelled.id, 2);
        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn cancel_last_order_clears_best() {
        let mut book = OrderBook::new();
        book.insert_order(ask(1, 105, 10, 1)).unwrap();
        book.cancel_order(1).unwrap();

        assert_eq!(book.best_ask(), None);
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn cancel_nonexistent_order() {
        let mut book = OrderBook::new();
        let err = book.cancel_order(999).unwrap_err();
        assert_eq!(err, BookError::OrderNotFound(999));
    }

    #[test]
    fn fifo_ordering_within_level() {
        let mut book = OrderBook::new();
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 100, 20, 2)).unwrap();
        book.insert_order(bid(3, 100, 30, 3)).unwrap();

        let front = book.peek_front(Side::Bid, 100).unwrap();
        assert_eq!(front.id, 1);
    }

    #[test]
    fn reduce_front_partial() {
        let mut book = OrderBook::new();
        book.insert_order(ask(1, 105, 100, 1)).unwrap();

        let remaining = book.reduce_front_quantity(Side::Ask, 105, 40).unwrap();
        assert_eq!(remaining, 60);
        assert_eq!(book.order_count(), 1);

        let front = book.peek_front(Side::Ask, 105).unwrap();
        assert_eq!(front.quantity, 60);
    }

    #[test]
    fn reduce_front_full_removes_order() {
        let mut book = OrderBook::new();
        book.insert_order(ask(1, 105, 100, 1)).unwrap();
        book.insert_order(ask(2, 105, 50, 2)).unwrap();

        let remaining = book.reduce_front_quantity(Side::Ask, 105, 100).unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(book.order_count(), 1);

        let front = book.peek_front(Side::Ask, 105).unwrap();
        assert_eq!(front.id, 2);
    }

    #[test]
    fn reduce_front_removes_empty_level() {
        let mut book = OrderBook::new();
        book.insert_order(ask(1, 105, 100, 1)).unwrap();
        book.insert_order(ask(2, 110, 50, 2)).unwrap();

        book.reduce_front_quantity(Side::Ask, 105, 100).unwrap();
        assert_eq!(book.best_ask(), Some(110));
    }

    #[test]
    fn fill_exceeds_quantity_error() {
        let mut book = OrderBook::new();
        book.insert_order(bid(1, 100, 10, 1)).unwrap();

        let err = book.reduce_front_quantity(Side::Bid, 100, 20).unwrap_err();
        assert_eq!(
            err,
            BookError::FillExceedsQuantity {
                available: 10,
                requested: 20
            }
        );
    }

    #[test]
    fn empty_book_defaults() {
        let book = OrderBook::new();
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.order_count(), 0);
        assert!(book.peek_front(Side::Bid, 100).is_none());
    }
}
