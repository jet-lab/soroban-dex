#![cfg_attr(not(test), no_std)]
#![feature(return_position_impl_trait_in_trait)]
#![allow(refining_impl_trait)]
#![allow(private_interfaces)]

mod storage;

use soroban_sdk::contracttype;
pub use storage::*;

/// A general purpose order book
pub struct OrderBook<T, B>
where
    T: 'static,
    B: Book<T>,
{
    _detail: core::marker::PhantomData<T>,
    book: B,
}

impl<T, B> OrderBook<T, B>
where
    B: Book<T>,
    T: 'static,
{
    pub fn new(book: B) -> Self {
        Self {
            _detail: core::marker::PhantomData,
            book,
        }
    }

    pub fn get_order(&self, id: &B::OrderId) -> Option<OrderEntry<B::OrderId, T>> {
        self.book.get_order(id)
    }

    pub fn orders(&self, side: OrderSide) -> impl IntoIterator<Item = B::OrderId> + '_ {
        self.book.orders(side)
    }

    pub fn cancel_order(&self, id: &B::OrderId) {
        self.book.remove_order(id);
    }

    pub fn place_order(
        &self,
        params: &OrderParams<T>,
        mut on_match: impl FnMut(&OrderEntry<B::OrderId, T>),
    ) -> OrderSummary<B::OrderId> {
        let matchable = self.book.orders(params.side.opposite());
        let mut amount_to_post = params.size;

        for order_id in matchable {
            let Some(order) = self.book.get_order(&order_id) else {
                continue;
            };

            let is_matching = match params.side {
                OrderSide::Bid => order.price <= params.price,
                OrderSide::Ask => order.price >= params.price,
            };

            if !is_matching {
                break;
            }

            if amount_to_post >= order.size {
                amount_to_post -= order.size;

                self.book.remove_order(&order_id);
                on_match(&order);
            } else {
                let order_new_size = order.size - amount_to_post;

                self.book.modify_order(&order_id, order_new_size);
                amount_to_post = 0;

                on_match(&OrderEntry {
                    size: order_new_size,
                    ..order
                });
            }

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
}

/// An interface to the storage of an order book
pub trait Book<T: 'static> {
    type OrderId: Eq + Clone;

    fn get_order(&self, id: &Self::OrderId) -> Option<OrderEntry<Self::OrderId, T>>;
    fn orders(&self, side: OrderSide) -> impl IntoIterator<Item = Self::OrderId>;
    fn place_order(&self, side: OrderSide, price: u64, size: u128, details: &T) -> Self::OrderId;
    fn remove_order(&self, id: &Self::OrderId);
    fn modify_order(&self, id: &Self::OrderId, new_size: u128);
}

/// The side of the book an order can be placed on
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum OrderSide {
    Bid = 0,
    Ask = 1,
}

impl OrderSide {
    pub fn opposite(&self) -> Self {
        match self {
            OrderSide::Bid => OrderSide::Ask,
            OrderSide::Ask => OrderSide::Bid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderParams<T: 'static> {
    pub side: OrderSide,
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
