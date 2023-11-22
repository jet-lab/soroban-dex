#![cfg_attr(not(test), no_std)]
#![allow(refining_impl_trait)]
#![allow(private_interfaces)]

mod orders;
mod storage;

pub use orders::*;
use soroban_sdk::{contracttype, Env, IntoVal, Map, TryFromVal, Val, Vec};
use storage::*;

/// A general purpose order book
pub struct OrderBook<T>
where
    T: 'static,
{
    _detail: core::marker::PhantomData<T>,
    book: BookStorage,
}

impl<T> OrderBook<T>
where
    T: TryFromVal<Env, Val> + IntoVal<Env, Val> + 'static,
{
    /// Open an orderbook structure within the current environment
    ///
    /// # Params
    ///
    /// `prefix` - An identifier which is used as a prefix for all keys that will be used
    ///            to store data for the order book.
    pub fn open(env: &Env, prefix: u16) -> Self {
        Self {
            _detail: core::marker::PhantomData,
            book: BookStorage::new(env, prefix),
        }
    }

    pub fn get_order(&self, id: &OrderId) -> Option<OrderEntry<OrderId, T>> {
        self.book().get_order(id)
    }

    pub fn orders(&self, side: OrderbookSide) -> impl IntoIterator<Item = OrderId> + '_ {
        self.book().orders(side)
    }

    pub fn cancel_order(&self, id: &OrderId) {
        self.book().remove_order(id);
    }

    pub fn place_order(
        &self,
        params: &OrderParams<T>,
        mut on_match: impl FnMut(&OrderEntry<OrderId, T>),
    ) -> OrderSummary<OrderId> {
        let matchable = self.book().orders(params.side.opposite());
        let order_events = self.book().order_events();
        let mut amount_to_post = params.size;

        for order_id in matchable {
            let Some(order) = self.book.get_order(&order_id) else {
                continue;
            };

            let is_matching = match params.side {
                OrderbookSide::Bid => order.price <= params.price,
                OrderbookSide::Ask => order.price >= params.price,
            };

            if !is_matching {
                break;
            }

            let matched_size = match order.size {
                size if size <= amount_to_post => {
                    self.book().modify_order(&order_id, 0);
                    size
                }

                size => {
                    self.book().modify_order(&order_id, size - amount_to_post);
                    amount_to_post
                }
            };

            amount_to_post -= matched_size;

            order_events.push(&order.id, OrderEvent::Fill(matched_size));

            on_match(&OrderEntry {
                size: matched_size,
                ..order
            });

            if amount_to_post == 0 {
                break;
            }
        }

        let mut posted_id = None;
        if amount_to_post > 0 {
            posted_id = Some(self.book.place_order(
                params.side,
                params.price,
                amount_to_post,
                &params.details,
            ));
        }

        OrderSummary {
            posted_id,
            posted_size: amount_to_post,
        }
    }

    pub fn events(&self) -> Map<OrderId, Vec<OrderEvent>> {
        self.book().order_events().all()
    }

    pub fn consume_events(&self, orders: Map<OrderId, u32>) -> Vec<(OrderId, OrderEvent)> {
        self.book().order_events().consume(orders)
    }

    fn book(&self) -> &impl Book<T> {
        &self.book
    }
}

/// An interface to the storage of an order book
pub trait Book<T: 'static> {
    fn get_order(&self, id: &OrderId) -> Option<OrderEntry<OrderId, T>>;
    fn orders(&self, side: OrderbookSide) -> impl IntoIterator<Item = OrderId>;
    fn place_order(&self, side: OrderbookSide, price: u64, size: u128, details: &T) -> OrderId;
    fn remove_order(&self, id: &OrderId);
    fn modify_order(&self, id: &OrderId, new_size: u128);
    fn order_events(&self) -> impl OrderEventMap;
}

pub trait OrderEventMap {
    fn all(&self) -> Map<OrderId, Vec<OrderEvent>>;
    fn get(&self, order: &OrderId) -> Vec<OrderEvent>;
    fn push(&self, order: &OrderId, event: OrderEvent);
    fn consume(&self, orders: Map<OrderId, u32>) -> Vec<(OrderId, OrderEvent)>;
}

/// The side of the book an order can be placed on
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum OrderbookSide {
    Bid = 0,
    Ask = 1,
}

impl OrderbookSide {
    pub fn opposite(&self) -> Self {
        match self {
            OrderbookSide::Bid => OrderbookSide::Ask,
            OrderbookSide::Ask => OrderbookSide::Bid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderParams<T: 'static> {
    pub side: OrderbookSide,
    pub price: u64,
    pub size: u128,
    pub details: T,
}

/// An order stored in a book
#[derive(Clone)]
pub struct OrderEntry<Id, T>
where
    Id: Eq + Clone,
{
    pub id: Id,
    pub price: u64,
    pub size: u128,
    pub details: T,
}

/// The summary provided after attempting to post an order
#[derive(Clone)]
pub struct OrderSummary<Id>
where
    Id: Eq + Clone,
{
    /// The order ID of the new order in the book
    pub posted_id: Option<Id>,

    /// The size of the order that was posted
    pub posted_size: u128,
}
