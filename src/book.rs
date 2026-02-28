use std::collections::HashMap;

use crate::arena::{ARENA_NULL, Arena, ArenaError, OrderNode, PriceLevel};
use crate::order::{Order, Side};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    DuplicateOrderId(u64),
    OrderNotFound(u64),
    PriceLevelNotFound(i64),
    FillExceedsQuantity { available: u64, requested: u64 },
    ArenaFull,
}

impl From<ArenaError> for BookError {
    fn from(_: ArenaError) -> Self {
        Self::ArenaFull
    }
}

#[derive(Debug)]
pub struct OrderBook {
    bids: HashMap<i64, PriceLevel>,
    asks: HashMap<i64, PriceLevel>,
    best_bid: Option<i64>,
    best_ask: Option<i64>,
    order_index: HashMap<u64, u32>,
    arena: Arena,
}

impl OrderBook {
    pub fn new() -> Self {
        Self::with_capacity(Arena::default_capacity())
    }

    pub fn with_capacity(arena_capacity: u32) -> Self {
        Self {
            bids: HashMap::with_capacity(Arena::default_level_capacity()),
            asks: HashMap::with_capacity(Arena::default_level_capacity()),
            best_bid: None,
            best_ask: None,
            order_index: HashMap::with_capacity(arena_capacity as usize),
            arena: Arena::new(arena_capacity),
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

        let side = order.side;
        let price = order.price;
        let id = order.id;

        let Self {
            bids,
            asks,
            arena,
            order_index,
            ..
        } = self;

        let index = arena.alloc(&order)?;

        let levels = match side {
            Side::Bid => bids,
            Side::Ask => asks,
        };
        let level = levels.entry(price).or_insert_with(PriceLevel::new);
        arena.push_back(level, index);

        order_index.insert(id, index);

        self.update_best_after_insert(side, price);

        debug_assert_eq!(self.arena.count() as usize, self.order_index.len());
        Ok(())
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<Order, BookError> {
        let Self {
            bids,
            asks,
            arena,
            order_index,
            best_bid,
            best_ask,
        } = self;

        let index = order_index
            .remove(&order_id)
            .ok_or(BookError::OrderNotFound(order_id))?;

        let order = arena.get(index).to_order();
        let side = order.side;
        let price = order.price;

        let level_empty = {
            let level = match side {
                Side::Bid => bids.get_mut(&price),
                Side::Ask => asks.get_mut(&price),
            }
            .ok_or(BookError::PriceLevelNotFound(price))?;

            arena.remove(level, index);
            arena.dealloc(index);
            level.count == 0
        };

        if level_empty {
            match side {
                Side::Bid => {
                    bids.remove(&price);
                    *best_bid = bids.keys().copied().max();
                }
                Side::Ask => {
                    asks.remove(&price);
                    *best_ask = asks.keys().copied().min();
                }
            }
        }

        debug_assert_eq!(arena.count() as usize, order_index.len());
        Ok(order)
    }

    pub(crate) fn peek_front(&self, side: Side, price: i64) -> Option<&OrderNode> {
        let levels = match side {
            Side::Bid => &self.bids,
            Side::Ask => &self.asks,
        };
        let level = levels.get(&price)?;
        if level.head == ARENA_NULL {
            return None;
        }
        Some(self.arena.get(level.head))
    }

    pub(crate) fn reduce_front_quantity(
        &mut self,
        side: Side,
        price: i64,
        fill_qty: u64,
    ) -> Result<u64, BookError> {
        let Self {
            bids,
            asks,
            arena,
            order_index,
            best_bid,
            best_ask,
        } = self;

        let (remaining, level_empty) = {
            let level = match side {
                Side::Bid => bids.get_mut(&price),
                Side::Ask => asks.get_mut(&price),
            }
            .ok_or(BookError::PriceLevelNotFound(price))?;

            if level.head == ARENA_NULL {
                return Err(BookError::PriceLevelNotFound(price));
            }

            let head_idx = level.head;
            let front = arena.get_mut(head_idx);

            if fill_qty > front.quantity {
                return Err(BookError::FillExceedsQuantity {
                    available: front.quantity,
                    requested: fill_qty,
                });
            }

            front.quantity -= fill_qty;
            level.qty -= fill_qty;
            let remaining = front.quantity;

            if remaining == 0 {
                let removed_id = arena.get(head_idx).id;
                arena.pop_front(level);
                arena.dealloc(head_idx);
                order_index.remove(&removed_id);
                (0u64, level.count == 0)
            } else {
                (remaining, false)
            }
        };

        if level_empty {
            match side {
                Side::Bid => {
                    bids.remove(&price);
                    *best_bid = bids.keys().copied().max();
                }
                Side::Ask => {
                    asks.remove(&price);
                    *best_ask = asks.keys().copied().min();
                }
            }
        }

        debug_assert_eq!(arena.count() as usize, order_index.len());
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
        Order::new(id, 1, Side::Bid, price, qty, ts).unwrap()
    }

    fn ask(id: u64, price: i64, qty: u64, ts: u64) -> Order {
        Order::new(id, 1, Side::Ask, price, qty, ts).unwrap()
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

    #[test]
    fn arena_full_rejects_insert() {
        let mut book = OrderBook::with_capacity(2);
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 101, 10, 2)).unwrap();
        let err = book.insert_order(bid(3, 102, 10, 3)).unwrap_err();
        assert_eq!(err, BookError::ArenaFull);
        assert_eq!(book.order_count(), 2);
    }

    #[test]
    fn cancel_frees_slot_for_reuse() {
        let mut book = OrderBook::with_capacity(2);
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 101, 10, 2)).unwrap();
        assert_eq!(
            book.insert_order(bid(3, 102, 10, 3)).unwrap_err(),
            BookError::ArenaFull
        );

        book.cancel_order(1).unwrap();
        book.insert_order(bid(3, 102, 10, 3)).unwrap();
        assert_eq!(book.order_count(), 2);
        assert_eq!(book.best_bid(), Some(102));
    }

    #[test]
    fn cancel_middle_of_level() {
        let mut book = OrderBook::with_capacity(8);
        book.insert_order(bid(1, 100, 10, 1)).unwrap();
        book.insert_order(bid(2, 100, 20, 2)).unwrap();
        book.insert_order(bid(3, 100, 30, 3)).unwrap();

        book.cancel_order(2).unwrap();
        assert_eq!(book.order_count(), 2);

        let front = book.peek_front(Side::Bid, 100).unwrap();
        assert_eq!(front.id, 1);

        book.reduce_front_quantity(Side::Bid, 100, 10).unwrap();
        let front = book.peek_front(Side::Bid, 100).unwrap();
        assert_eq!(front.id, 3);
    }
}
