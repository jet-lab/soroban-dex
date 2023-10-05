#![no_std]

use fixed::types::U96F32;
use orderbook::{BookStorage, OrderBook};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol,
};

pub use orderbook::OrderId;

/// Specifies the side of the book an order is placed on
#[contracttype]
pub enum OrderSide {
    Bid,
    Ask,
}

/// The parameters for an order
#[contracttype]
pub struct OrderParams {
    /// The order type
    pub side: OrderSide,

    /// The size of the order (in base tokens)
    pub size: u128,

    /// The price of the order (U32F32 format) (in quote tokens)
    pub price: u64,

    /// The owning address of the order
    pub owner: Address,
}

/// The configuration for a trading market
#[contracttype]
pub struct DexMarketInfo {
    /// The token address for the base asset
    pub base_token: Address,

    /// The token address for the quote address (price)
    pub quote_token: Address,

    /// The minimum order size
    pub base_min_order_size: u128,
}

pub trait DexMarket {
    type Error;

    fn init(env: Env, info: DexMarketInfo);
    fn place_order(env: Env, params: OrderParams) -> Result<Option<OrderId>, Self::Error>;
    fn cancel_order(env: Env, order: OrderId);
}

impl From<OrderSide> for orderbook::OrderSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Bid => orderbook::OrderSide::Bid,
            OrderSide::Ask => orderbook::OrderSide::Ask,
        }
    }
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DexMarketError {
    InvalidOrderSize = 100,
    CannotSelfTrade = 101,
}

#[contract]
pub struct DexMarketContract;

#[contractimpl]
impl DexMarket for DexMarketContract {
    type Error = DexMarketError;

    /// Initialize a new market
    fn init(env: Env, info: DexMarketInfo) {
        env.storage().instance().set(&MARKET_INFO, &info);
    }

    /// Place a new order in the market
    fn place_order(env: Env, params: OrderParams) -> Result<Option<OrderId>, DexMarketError> {
        use orderbook::OrderSide;

        let order_book = order_book_state(&env);
        let params = orderbook::OrderParams {
            side: params.side.into(),
            size: params.size,
            price: params.price,
            details: OrderDetail {
                owner: params.owner,
            },
        };

        let market_info: DexMarketInfo = env.storage().instance().get(&MARKET_INFO).unwrap();
        let base = token::Client::new(&env, &market_info.base_token);
        let quote = token::Client::new(&env, &market_info.quote_token);

        if params.size < market_info.base_min_order_size {
            return Err(DexMarketError::InvalidOrderSize);
        }

        match params.side {
            OrderSide::Bid => {
                let quote_offer_amount =
                    U96F32::from_bits(params.price as u128) * U96F32::from_num(params.size);

                quote.transfer(
                    &params.details.owner,
                    &env.current_contract_address(),
                    &quote_offer_amount.to_num(),
                );
            }

            OrderSide::Ask => {
                base.transfer(
                    &params.details.owner,
                    &env.current_contract_address(),
                    &(params.size as i128),
                );
            }
        }

        let mut is_self_trade = false;
        let summary = order_book.place_order(&params, |entry| {
            is_self_trade = is_self_trade || entry.details.owner == params.details.owner;

            match entry.id.side() {
                OrderSide::Bid => {
                    base.transfer(
                        &env.current_contract_address(),
                        &entry.details.owner,
                        &(entry.size as i128),
                    );
                }

                OrderSide::Ask => {
                    let price = U96F32::from_bits(entry.price as u128);
                    let token_amount = price * U96F32::from_num(entry.size);

                    quote.transfer(
                        &env.current_contract_address(),
                        &entry.details.owner,
                        &token_amount.to_num(),
                    );
                }
            }
        });

        if is_self_trade {
            return Err(DexMarketError::CannotSelfTrade);
        }

        Ok(summary.posted_id)
    }

    /// Cancel a previously placed order
    fn cancel_order(env: Env, order: OrderId) {
        use orderbook::OrderSide;

        let order_book = order_book_state(&env);
        let order_detail = order_book.get_order(&order);

        if let Some(order_detail) = order_detail {
            order_detail.details.owner.require_auth();

            let market_info: DexMarketInfo = env.storage().instance().get(&MARKET_INFO).unwrap();
            let base = token::Client::new(&env, &market_info.base_token);
            let quote = token::Client::new(&env, &market_info.quote_token);

            match order_detail.id.side() {
                OrderSide::Ask => {
                    base.transfer(
                        &env.current_contract_address(),
                        &order_detail.details.owner,
                        &(order_detail.size as i128),
                    );
                }

                OrderSide::Bid => {
                    let price = U96F32::from_bits(order_detail.price as u128);
                    let token_amount = price * U96F32::from_num(order_detail.size);

                    quote.transfer(
                        &env.current_contract_address(),
                        &order_detail.details.owner,
                        &token_amount.to_num(),
                    );
                }
            }

            order_book.cancel_order(&order);
        }
    }
}

fn order_book_state(env: &Env) -> OrderBook<OrderDetail, BookStorage> {
    OrderBook::new(BookStorage::open(&env, 0xF1A0))
}

#[contracttype]
struct OrderDetail {
    owner: Address,
}

const MARKET_INFO: Symbol = symbol_short!("MARKETINF");
