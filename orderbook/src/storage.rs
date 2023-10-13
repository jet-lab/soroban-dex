use soroban_sdk::{storage::Persistent, Bytes, Env, IntoVal, Map, TryFromVal, Val, Vec};

use crate::{Book, OrderEntry, OrderEvent, OrderEventMap, OrderId, OrderSide};

/// Provides an order book storage interface within a Soroban contract environment
#[derive(Clone)]
pub struct BookStorage {
    prefix: u16,
    env: Env,
}

impl BookStorage {
    /// Open the interface within the current environment
    ///
    /// # Params
    ///
    /// `prefix` - An identifier which is used as a prefix for all keys that will be used
    ///            to store data for the order book.
    pub fn new(env: &Env, prefix: u16) -> Self {
        Self {
            prefix,
            env: env.clone(),
        }
    }

    fn storage(&self) -> Persistent {
        self.env.storage().persistent()
    }

    fn order_events_key(&self) -> Bytes {
        let mut key = Bytes::from_array(&self.env, &self.prefix.to_be_bytes());
        key.push_back(0xFF);

        key
    }

    fn book_key(&self, side: OrderSide) -> Bytes {
        let mut book_key = Bytes::from_array(&self.env, &self.prefix.to_be_bytes());
        book_key.push_back(side as u8);

        book_key
    }

    fn get_book(&self, side: OrderSide) -> Map<u64, ()> {
        let key = self.book_key(side);

        self.storage()
            .get::<Bytes, Map<u64, ()>>(&key)
            .unwrap_or_else(|| Map::new(&self.env))
    }

    fn set_book(&self, side: OrderSide, book: &Map<u64, ()>) {
        let key = self.book_key(side);
        self.storage().set(&key, book);
    }

    fn price_queue_key(&self, price: u64) -> Bytes {
        OrderId::new(&self.env, self.prefix, OrderSide::Bid, price, 0).price_key()
    }

    fn get_price_queue(&self, price: u64) -> Map<u32, u128> {
        let price_key = self.price_queue_key(price);
        self.env
            .storage()
            .persistent()
            .get::<Bytes, Map<u32, u128>>(&price_key)
            .unwrap_or_else(|| Map::new(&self.env))
    }

    fn set_price_queue(&self, price: u64, queue: &Map<u32, u128>) {
        let price_key = self.price_queue_key(price);
        self.env.storage().persistent().set(&price_key, queue)
    }

    fn cleanup_order(&self, order: &OrderId, force_remove: bool) {
        let mut queue = self.get_price_queue(order.price());
        let current_size = queue.get(order.id()).unwrap_or(0);

        if current_size > 0 && !force_remove {
            return;
        }

        queue.remove(order.id());
        self.storage().remove(order);

        let price = order.price();

        match queue.is_empty() {
            false => self.set_price_queue(price, &queue),
            true => {
                self.storage().remove(&self.price_queue_key(price));

                // since the order queue is empty for the price now, also remove
                // the price from the root list
                let mut book = self.get_book(order.side());
                book.remove(price);

                self.set_book(order.side(), &book);
            }
        }
    }
}

impl<T> Book<T> for BookStorage
where
    T: TryFromVal<Env, Val> + IntoVal<Env, Val> + 'static,
{
    fn get_order(&self, id: &OrderId) -> Option<OrderEntry<OrderId, T>> {
        let queue = self.get_price_queue(id.price());
        let size = queue.get(id.id())?;

        self.storage()
            .get::<OrderId, T>(&id)
            .map(|details| OrderEntry {
                id: id.clone(),
                price: id.price(),
                size,
                details,
            })
    }

    fn orders(&self, side: OrderSide) -> StoredOrders {
        let book = self.get_book(side);

        match side {
            OrderSide::Bid => StoredOrders::bids(self, book.keys().into_iter().rev()),
            OrderSide::Ask => StoredOrders::asks(self, book.keys().into_iter()),
        }
    }

    fn place_order(&self, side: OrderSide, price: u64, size: u128, details: &T) -> OrderId {
        // update book price list
        let mut book = self.get_book(side);

        if !book.contains_key(price) {
            book.set(price, ());
            self.set_book(side, &book);
        }

        // update price order queue
        let mut queue = self.get_price_queue(price);
        let next_local_id = queue.keys().last().map(|id| id + 1).unwrap_or(0);

        queue.set(next_local_id, size);
        self.set_price_queue(price, &queue);

        // set order entry
        let order_id = OrderId::new(&self.env, self.prefix, side, price, next_local_id);
        self.storage().set(&order_id, details);

        order_id
    }

    fn remove_order(&self, id: &OrderId) {
        self.cleanup_order(id, true)
    }

    fn modify_order(&self, id: &OrderId, size: u128) {
        let mut queue = self.get_price_queue(id.price());
        queue.set(id.id(), size);

        self.set_price_queue(id.price(), &queue);
    }

    fn order_events(&self) -> impl OrderEventMap {
        OrderEventQueue::new(self.clone())
    }
}

struct OrderEventQueue {
    inner: BookStorage,
}

impl OrderEventQueue {
    fn new(storage: BookStorage) -> Self {
        Self { inner: storage }
    }
}

impl OrderEventMap for OrderEventQueue {
    fn all(&self) -> Map<OrderId, Vec<OrderEvent>> {
        let key = self.inner.order_events_key();
        self.inner
            .storage()
            .get::<Bytes, Map<OrderId, Vec<OrderEvent>>>(&key)
            .unwrap_or_else(|| Map::new(&self.inner.env))
    }

    fn get(&self, order: &OrderId) -> Vec<crate::OrderEvent> {
        self.all()
            .get(order.clone())
            .unwrap_or_else(|| Vec::new(&self.inner.env))
    }

    fn push(&self, order: &OrderId, event: OrderEvent) {
        let key = self.inner.order_events_key();
        let mut map = self.all();

        let mut events = map
            .get(order.clone())
            .unwrap_or_else(|| Vec::new(&self.inner.env));

        events.push_back(event);
        map.set(order.clone(), events);

        self.inner.storage().set(&key, &map);
    }

    fn consume(&self, orders: Map<OrderId, u32>) -> Vec<(OrderId, OrderEvent)> {
        let mut map = self.all();
        let mut to_consume = Vec::new(&self.inner.env);

        for (order, count) in orders {
            let count = count as usize;

            let mut events = map
                .get(order.clone())
                .unwrap_or_else(|| Vec::new(&self.inner.env));

            for _ in 0..count {
                if let Some(next) = events.pop_front() {
                    to_consume.push_back((order.clone(), next));
                }
            }

            match events.len() {
                0 => {
                    map.remove(order.clone());
                    self.inner.cleanup_order(&order, false);
                }
                _ => _ = map.set(order.clone(), events),
            }
        }

        let key = self.inner.order_events_key();
        self.inner.storage().set(&key, &map);

        to_consume
    }
}

struct StoredOrders {
    storage: BookStorage,
    inner: StoredOrdersInner,
    side: OrderSide,
    current_price: u64,
    current_queue: Option<Vec<u32>>,
}

impl StoredOrders {
    fn bids(
        storage: &BookStorage,
        prices: core::iter::Rev<<Vec<u64> as IntoIterator>::IntoIter>,
    ) -> Self {
        Self {
            storage: storage.clone(),
            inner: StoredOrdersInner::BidPrices(prices),
            side: OrderSide::Bid,
            current_price: 0,
            current_queue: None,
        }
    }

    fn asks(storage: &BookStorage, prices: <Vec<u64> as IntoIterator>::IntoIter) -> Self {
        Self {
            storage: storage.clone(),
            inner: StoredOrdersInner::AskPrices(prices),
            side: OrderSide::Ask,
            current_price: 0,
            current_queue: None,
        }
    }
}

enum StoredOrdersInner {
    BidPrices(core::iter::Rev<<Vec<u64> as IntoIterator>::IntoIter>),
    AskPrices(<Vec<u64> as IntoIterator>::IntoIter),
}

impl Iterator for StoredOrders {
    type Item = OrderId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match &mut self.current_queue {
                None => {
                    let price = match &mut self.inner {
                        StoredOrdersInner::BidPrices(prices) => prices.next(),
                        StoredOrdersInner::AskPrices(prices) => prices.next(),
                    };

                    let Some(price) = price else {
                        return None;
                    };

                    self.current_price = price;
                    self.current_queue = Some(self.storage.get_price_queue(price).keys());
                }

                Some(queue) => {
                    let local_order_id = match queue.pop_front() {
                        Some(id) => id,
                        None => {
                            self.current_queue = None;
                            continue;
                        }
                    };

                    return Some(OrderId::new(
                        &self.storage.env,
                        self.storage.prefix,
                        self.side,
                        self.current_price,
                        local_order_id,
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use soroban_sdk::{contract, contractimpl, Address};

    #[contract]
    struct Contract;

    #[contractimpl]
    impl Contract {
        fn book(env: &Env) -> impl Book<u64> {
            BookStorage::new(env, 0xBEEF)
        }

        pub fn place_bid(env: Env, price: u64, size: u128) -> OrderId {
            let book = Self::book(&env);

            book.place_order(OrderSide::Bid, price, size, &0)
        }

        pub fn place_ask(env: Env, price: u64, size: u128) -> OrderId {
            let book = Self::book(&env);

            book.place_order(OrderSide::Ask, price, size, &0)
        }

        pub fn remove_order(env: Env, id: OrderId) {
            let book = Self::book(&env);

            book.remove_order(&id);
        }

        pub fn get_order_size(env: Env, id: OrderId) -> Option<u128> {
            Self::book(&env).get_order(&id).map(|o| o.size)
        }

        pub fn top_bid(env: Env) -> Option<OrderId> {
            let book = Self::book(&env);

            let mut orders = book.orders(OrderSide::Bid).into_iter();
            orders.next().map(|id| id.clone())
        }

        pub fn top_ask(env: Env) -> Option<OrderId> {
            let book = Self::book(&env);

            let mut orders = book.orders(OrderSide::Ask).into_iter();
            orders.next().map(|id| id.clone())
        }
    }

    struct TestEnv {
        env: Env,
        contract_id: Address,
    }

    impl TestEnv {
        fn new() -> Self {
            let env = Env::default();

            Self {
                contract_id: env.register_contract(None, Contract),
                env,
            }
        }

        fn client(&self) -> ContractClient {
            ContractClient::new(&self.env, &self.contract_id)
        }
    }

    #[test]
    fn can_place_remove_orders() {
        let env = TestEnv::new();
        let client = env.client();

        let orders = [
            client.place_bid(&125, &20),
            client.place_bid(&150, &30),
            client.place_bid(&100, &10),
            client.place_ask(&250, &50),
            client.place_ask(&200, &10),
            client.place_ask(&225, &20),
        ];

        assert_eq!(150, client.top_bid().unwrap().price());
        assert_eq!(200, client.top_ask().unwrap().price());
        assert_eq!(Some(30), client.get_order_size(&orders[1]));
        assert_eq!(Some(10), client.get_order_size(&orders[4]));

        client.remove_order(&orders[1]);
        client.remove_order(&orders[4]);

        assert_eq!(125, client.top_bid().unwrap().price());
        assert_eq!(225, client.top_ask().unwrap().price());
    }

    #[test]
    fn orders_at_same_price_will_queue() {
        let env = TestEnv::new();
        let client = env.client();

        let mut bids = vec![];
        let mut asks = vec![];

        env.env.budget().reset_unlimited();

        for i in 1..100 {
            let size = 1000 + i * 5;
            bids.push(client.place_bid(&100, &size));
            asks.push(client.place_ask(&200, &size));
        }

        for i in 1..100 {
            let size = 1000 + i * 5;
            let bid = client.top_bid().unwrap();
            let ask = client.top_ask().unwrap();

            assert_eq!(Some(size), client.get_order_size(&bid));
            assert_eq!(Some(size), client.get_order_size(&ask));

            client.remove_order(&bid);
            client.remove_order(&ask);
        }
    }
}
