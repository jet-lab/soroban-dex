use core::fmt::Debug;

use soroban_sdk::{
    contracttype, storage::Persistent, Bytes, BytesN, Env, IntoVal, Map, TryFromVal, Val, Vec,
};

use crate::{Book, OrderEntry, OrderSide};

pub const ORDER_ATTR_KEY_DETAIL: u8 = 0;
pub const ORDER_ATTR_KEY_SIZE: u8 = 1;

/// An identifier for an order in the book
///
/// This is also the key for the order in the contract storage
///
/// Structure:
///     - 2 bytes: prefix (for contract storage namespacing)
///     - 1 byte: order side
///     - 1 byte: order attribute keyspace
///     - 8 bytes: price (lists orders)
///     - 4 bytes: order id (a specific order entry)
#[contracttype]
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct OrderId(BytesN<16>);

impl OrderId {
    pub fn new(env: &Env, prefix: u16, side: OrderSide, price: u64, id: u32) -> Self {
        let mut bytes = [0u8; 16];
        bytes[0..2].copy_from_slice(&prefix.to_be_bytes());
        bytes[3] = side as u8;

        bytes[4..12].copy_from_slice(&price.to_be_bytes());
        bytes[12..16].copy_from_slice(&id.to_be_bytes());

        Self(BytesN::from_array(env, &bytes))
    }

    pub fn side(&self) -> OrderSide {
        match self.0.to_array()[3] {
            0 => OrderSide::Bid,
            1 => OrderSide::Ask,
            _ => unreachable!(),
        }
    }

    pub fn book_key(&self) -> Bytes {
        Bytes::from_slice(&self.0.env(), &self.0.to_array()[0..3])
    }

    pub fn price(&self) -> u64 {
        u64::from_be_bytes(self.0.to_array()[4..12].try_into().unwrap())
    }

    pub fn price_key(&self) -> Bytes {
        let mut bytes = Bytes::from_slice(&self.0.env(), &self.0.to_array()[0..12]);
        bytes.set(4, 0);

        bytes
    }

    pub fn id(&self) -> u32 {
        u32::from_be_bytes(self.0.to_array()[12..16].try_into().unwrap())
    }

    pub fn with_attr_key(&self, key: u8) -> Self {
        let mut bytes = self.0.to_array();
        bytes[4] = key;

        Self(BytesN::from_array(self.0.env(), &bytes))
    }
}

impl AsRef<BytesN<16>> for OrderId {
    fn as_ref(&self) -> &BytesN<16> {
        &self.0
    }
}

impl Debug for OrderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut bytes = [0u8; 48];
        hex::encode_to_slice(self.0.to_array(), &mut bytes).unwrap();

        write!(f, "{}", core::str::from_utf8(&bytes).unwrap())
    }
}

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
    pub fn open(env: &Env, prefix: u16) -> Self {
        Self {
            prefix,
            env: env.clone(),
        }
    }

    fn storage(&self) -> Persistent {
        self.env.storage().persistent()
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

    fn get_price_queue(&self, price: u64) -> Vec<u32> {
        let price_key = self.price_queue_key(price);
        self.env
            .storage()
            .persistent()
            .get::<Bytes, Vec<u32>>(&price_key)
            .unwrap_or_else(|| Vec::new(&self.env))
    }

    fn set_price_queue(&self, price: u64, queue: &Vec<u32>) {
        let price_key = self.price_queue_key(price);
        self.env.storage().persistent().set(&price_key, queue)
    }

    fn set_order_size(&self, id: &OrderId, size: u128) {
        let size_key = id.with_attr_key(ORDER_ATTR_KEY_SIZE);
        self.storage().set(&size_key, &size);
    }
}

impl<T> Book<T> for BookStorage
where
    T: TryFromVal<Env, Val> + IntoVal<Env, Val> + 'static,
{
    type OrderId = OrderId;

    fn get_order(&self, id: &Self::OrderId) -> Option<OrderEntry<Self::OrderId, T>> {
        let size_key = id.with_attr_key(ORDER_ATTR_KEY_SIZE);
        let size = self.storage().get::<OrderId, u128>(&size_key)?;

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

    fn place_order(&self, side: OrderSide, price: u64, size: u128, details: &T) -> Self::OrderId {
        // update book price list
        let mut book = self.get_book(side);

        if !book.contains_key(price) {
            book.set(price, ());
            self.set_book(side, &book);
        }

        // update price order queue
        let mut queue = self.get_price_queue(price);
        let next_local_id = queue.last().map(|id| id + 1).unwrap_or(0);

        queue.push_back(next_local_id);
        self.set_price_queue(price, &queue);

        // set order entry
        let order_id = OrderId::new(&self.env, self.prefix, side, price, next_local_id);
        self.storage().set(&order_id, details);

        // set order size
        self.set_order_size(&order_id, size);

        order_id
    }

    fn remove_order(&self, id: &Self::OrderId) {
        // remove detail and size
        self.storage().remove(id);
        self.storage()
            .remove(&id.with_attr_key(ORDER_ATTR_KEY_SIZE));

        // update price order queue
        let mut queue = self.get_price_queue(id.price());
        if let Some(index) = queue.first_index_of(id.id()) {
            queue.remove(index);
        }

        match queue.is_empty() {
            false => self.set_price_queue(id.price(), &queue),
            true => {
                self.storage().remove(&self.price_queue_key(id.price()));

                // since the order queue is empty for the price now, also remove
                // the price from the root list
                let mut book = self.get_book(id.side());
                book.remove(id.price());

                self.set_book(id.side(), &book);
            }
        }
    }

    fn modify_order(&self, id: &Self::OrderId, size: u128) {
        self.set_order_size(id, size)
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
                    self.current_queue = Some(self.storage.get_price_queue(price));
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
        fn book(env: &Env) -> impl Book<u64, OrderId = OrderId> {
            BookStorage::open(env, 0xBEEF)
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
