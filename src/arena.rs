use crate::order::{Order, Side};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArenaError {
    Full,
}

pub(crate) const ARENA_NULL: u32 = u32::MAX;

const DEFAULT_CAPACITY: u32 = 1_048_576;

#[derive(Clone)]
#[repr(C, align(64))]
pub(crate) struct OrderNode {
    pub(crate) id: u64,
    pub(crate) trader_id: u64,
    pub(crate) price: i64,
    pub(crate) quantity: u64,
    pub(crate) timestamp: u64,
    pub(crate) prev: u32,
    pub(crate) next: u32,
    pub(crate) side: Side,
    _pad: [u8; 15],
}

impl OrderNode {
    fn zeroed() -> Self {
        Self {
            id: 0,
            trader_id: 0,
            price: 0,
            quantity: 0,
            timestamp: 0,
            prev: ARENA_NULL,
            next: ARENA_NULL,
            side: Side::Bid,
            _pad: [0u8; 15],
        }
    }

    pub(crate) fn from_order(order: &Order) -> Self {
        Self {
            id: order.id,
            trader_id: order.trader_id,
            price: order.price,
            quantity: order.quantity,
            timestamp: order.timestamp,
            prev: ARENA_NULL,
            next: ARENA_NULL,
            side: order.side,
            _pad: [0u8; 15],
        }
    }

    pub(crate) fn to_order(&self) -> Order {
        Order {
            id: self.id,
            trader_id: self.trader_id,
            side: self.side,
            price: self.price,
            quantity: self.quantity,
            timestamp: self.timestamp,
        }
    }
}

impl std::fmt::Debug for OrderNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrderNode")
            .field("id", &self.id)
            .field("trader_id", &self.trader_id)
            .field("price", &self.price)
            .field("quantity", &self.quantity)
            .field("timestamp", &self.timestamp)
            .field("prev", &self.prev)
            .field("next", &self.next)
            .field("side", &self.side)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PriceLevel {
    pub(crate) head: u32,
    pub(crate) tail: u32,
    pub(crate) count: u32,
    pub(crate) qty: u64,
}

impl PriceLevel {
    pub(crate) fn new() -> Self {
        Self {
            head: ARENA_NULL,
            tail: ARENA_NULL,
            count: 0,
            qty: 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Arena {
    storage: Vec<OrderNode>,
    free_head: u32,
    count: u32,
    capacity: u32,
}

impl Arena {
    pub(crate) fn new(capacity: u32) -> Self {
        let mut storage = Vec::with_capacity(capacity as usize);
        for i in 0..capacity {
            let mut node = OrderNode::zeroed();
            node.next = if i + 1 < capacity { i + 1 } else { ARENA_NULL };
            storage.push(node);
        }
        Self {
            storage,
            free_head: if capacity > 0 { 0 } else { ARENA_NULL },
            count: 0,
            capacity,
        }
    }

    pub(crate) fn default_capacity() -> u32 {
        DEFAULT_CAPACITY
    }

    pub(crate) fn count(&self) -> u32 {
        self.count
    }

    pub(crate) fn alloc(&mut self, order: &Order) -> Result<u32, ArenaError> {
        if self.free_head == ARENA_NULL {
            return Err(ArenaError::Full);
        }

        let index = self.free_head;
        self.free_head = self.storage[index as usize].next;
        self.storage[index as usize] = OrderNode::from_order(order);
        self.count += 1;
        Ok(index)
    }

    pub(crate) fn dealloc(&mut self, index: u32) {
        debug_assert!(index < self.capacity);
        self.storage[index as usize].next = self.free_head;
        self.free_head = index;
        self.count -= 1;
    }

    pub(crate) fn get(&self, index: u32) -> &OrderNode {
        &self.storage[index as usize]
    }

    pub(crate) fn get_mut(&mut self, index: u32) -> &mut OrderNode {
        &mut self.storage[index as usize]
    }

    pub(crate) fn push_back(&mut self, level: &mut PriceLevel, index: u32) {
        let quantity = self.storage[index as usize].quantity;

        if level.tail != ARENA_NULL {
            let old_tail = level.tail;
            self.storage[old_tail as usize].next = index;
            self.storage[index as usize].prev = old_tail;
        } else {
            level.head = index;
            self.storage[index as usize].prev = ARENA_NULL;
        }

        self.storage[index as usize].next = ARENA_NULL;
        level.tail = index;
        level.count += 1;
        level.qty += quantity;
    }

    pub(crate) fn pop_front(&mut self, level: &mut PriceLevel) -> Option<u32> {
        if level.head == ARENA_NULL {
            return None;
        }

        let index = level.head;
        let next = self.storage[index as usize].next;
        let quantity = self.storage[index as usize].quantity;

        if next != ARENA_NULL {
            self.storage[next as usize].prev = ARENA_NULL;
            level.head = next;
        } else {
            level.head = ARENA_NULL;
            level.tail = ARENA_NULL;
        }

        level.count -= 1;
        level.qty -= quantity;
        Some(index)
    }

    pub(crate) fn remove(&mut self, level: &mut PriceLevel, index: u32) {
        let prev_idx = self.storage[index as usize].prev;
        let next_idx = self.storage[index as usize].next;
        let quantity = self.storage[index as usize].quantity;

        if prev_idx != ARENA_NULL {
            self.storage[prev_idx as usize].next = next_idx;
        } else {
            level.head = next_idx;
        }

        if next_idx != ARENA_NULL {
            self.storage[next_idx as usize].prev = prev_idx;
        } else {
            level.tail = prev_idx;
        }

        level.count -= 1;
        level.qty -= quantity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(id: u64, price: i64, qty: u64) -> Order {
        Order::new(id, id, Side::Bid, price, qty, id).unwrap()
    }

    #[test]
    fn ordernode_size_and_alignment() {
        assert_eq!(std::mem::size_of::<OrderNode>(), 64);
        assert_eq!(std::mem::align_of::<OrderNode>(), 64);
    }

    #[test]
    fn ordernode_roundtrip() {
        let order = Order::new(1, 2, Side::Ask, 100, 50, 999).unwrap();
        let node = OrderNode::from_order(&order);
        let back = node.to_order();
        assert_eq!(back, order);
    }

    #[test]
    fn arena_alloc_dealloc_cycle() {
        let mut arena = Arena::new(4);
        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 101, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 102, 30)).unwrap();
        let i3 = arena.alloc(&make_order(4, 103, 40)).unwrap();

        assert_eq!(arena.count(), 4);
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(i2, 2);
        assert_eq!(i3, 3);

        arena.dealloc(i1);
        arena.dealloc(i3);
        assert_eq!(arena.count(), 2);

        let i4 = arena.alloc(&make_order(5, 104, 50)).unwrap();
        let i5 = arena.alloc(&make_order(6, 105, 60)).unwrap();
        assert_eq!(arena.count(), 4);
        assert_eq!(i4, 3);
        assert_eq!(i5, 1);
    }

    #[test]
    fn arena_full() {
        let mut arena = Arena::new(2);
        arena.alloc(&make_order(1, 100, 10)).unwrap();
        arena.alloc(&make_order(2, 101, 20)).unwrap();
        assert_eq!(
            arena.alloc(&make_order(3, 102, 30)).unwrap_err(),
            ArenaError::Full
        );
    }

    #[test]
    fn arena_zero_capacity() {
        let mut arena = Arena::new(0);
        assert_eq!(
            arena.alloc(&make_order(1, 100, 10)).unwrap_err(),
            ArenaError::Full
        );
    }

    #[test]
    fn push_back_builds_list() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();

        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        assert_eq!(level.head, i0);
        assert_eq!(level.tail, i2);
        assert_eq!(level.count, 3);
        assert_eq!(level.qty, 60);

        assert_eq!(arena.get(i0).prev, ARENA_NULL);
        assert_eq!(arena.get(i0).next, i1);
        assert_eq!(arena.get(i1).prev, i0);
        assert_eq!(arena.get(i1).next, i2);
        assert_eq!(arena.get(i2).prev, i1);
        assert_eq!(arena.get(i2).next, ARENA_NULL);
    }

    #[test]
    fn pop_front_drains_list() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();

        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        let popped = arena.pop_front(&mut level).unwrap();
        assert_eq!(popped, i0);
        assert_eq!(level.head, i1);
        assert_eq!(level.count, 2);
        assert_eq!(level.qty, 50);
        assert_eq!(arena.get(i1).prev, ARENA_NULL);

        let popped = arena.pop_front(&mut level).unwrap();
        assert_eq!(popped, i1);
        assert_eq!(level.head, i2);
        assert_eq!(level.count, 1);

        let popped = arena.pop_front(&mut level).unwrap();
        assert_eq!(popped, i2);
        assert_eq!(level.head, ARENA_NULL);
        assert_eq!(level.tail, ARENA_NULL);
        assert_eq!(level.count, 0);
        assert_eq!(level.qty, 0);

        assert!(arena.pop_front(&mut level).is_none());
    }

    #[test]
    fn remove_head() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();
        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        arena.remove(&mut level, i0);
        assert_eq!(level.head, i1);
        assert_eq!(level.tail, i2);
        assert_eq!(level.count, 2);
        assert_eq!(level.qty, 50);
        assert_eq!(arena.get(i1).prev, ARENA_NULL);
    }

    #[test]
    fn remove_tail() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();
        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        arena.remove(&mut level, i2);
        assert_eq!(level.head, i0);
        assert_eq!(level.tail, i1);
        assert_eq!(level.count, 2);
        assert_eq!(level.qty, 30);
        assert_eq!(arena.get(i1).next, ARENA_NULL);
    }

    #[test]
    fn remove_middle() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();
        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        arena.remove(&mut level, i1);
        assert_eq!(level.head, i0);
        assert_eq!(level.tail, i2);
        assert_eq!(level.count, 2);
        assert_eq!(level.qty, 40);
        assert_eq!(arena.get(i0).next, i2);
        assert_eq!(arena.get(i2).prev, i0);
    }

    #[test]
    fn remove_only_node() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        arena.push_back(&mut level, i0);

        arena.remove(&mut level, i0);
        assert_eq!(level.head, ARENA_NULL);
        assert_eq!(level.tail, ARENA_NULL);
        assert_eq!(level.count, 0);
        assert_eq!(level.qty, 0);
    }

    #[test]
    fn walk_forward_and_backward() {
        let mut arena = Arena::new(8);
        let mut level = PriceLevel::new();
        let ids: Vec<u64> = (1..=5).collect();
        let indices: Vec<u32> = ids
            .iter()
            .map(|&id| {
                let idx = arena.alloc(&make_order(id, 100, id)).unwrap();
                arena.push_back(&mut level, idx);
                idx
            })
            .collect();

        let mut forward = Vec::new();
        let mut cur = level.head;
        while cur != ARENA_NULL {
            forward.push(arena.get(cur).id);
            cur = arena.get(cur).next;
        }
        assert_eq!(forward, ids);

        let mut backward = Vec::new();
        cur = level.tail;
        while cur != ARENA_NULL {
            backward.push(arena.get(cur).id);
            cur = arena.get(cur).prev;
        }
        let reversed: Vec<u64> = ids.iter().rev().copied().collect();
        assert_eq!(backward, reversed);

        let _ = indices;
    }

    #[test]
    fn alloc_after_dealloc_reuses_slots() {
        let mut arena = Arena::new(3);
        let mut level = PriceLevel::new();

        let i0 = arena.alloc(&make_order(1, 100, 10)).unwrap();
        let i1 = arena.alloc(&make_order(2, 100, 20)).unwrap();
        let i2 = arena.alloc(&make_order(3, 100, 30)).unwrap();
        arena.push_back(&mut level, i0);
        arena.push_back(&mut level, i1);
        arena.push_back(&mut level, i2);

        arena.remove(&mut level, i1);
        arena.dealloc(i1);

        let i3 = arena.alloc(&make_order(4, 100, 40)).unwrap();
        assert_eq!(i3, i1);
        assert_eq!(arena.get(i3).id, 4);
        assert_eq!(arena.count(), 3);
    }
}
