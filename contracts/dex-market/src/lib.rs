#![cfg_attr(not(test), no_std)]

use fixed::types::U96F32;
use orderbook::OrderBook;
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, Map,
    Symbol,
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

impl From<OrderSide> for orderbook::OrderbookSide {
    fn from(value: OrderSide) -> Self {
        match value {
            OrderSide::Bid => orderbook::OrderbookSide::Bid,
            OrderSide::Ask => orderbook::OrderbookSide::Ask,
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
        use orderbook::OrderbookSide;

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

        params.details.owner.require_auth();
        let quote_offer_amount = quote_amount(params.price, params.size);

        match params.side {
            OrderbookSide::Bid => {
                quote.transfer(
                    &params.details.owner,
                    &env.current_contract_address(),
                    &quote_offer_amount,
                );
            }

            OrderbookSide::Ask => {
                base.transfer(
                    &params.details.owner,
                    &env.current_contract_address(),
                    &(params.size as i128),
                );
            }
        }

        let mut quote_consumed = 0;
        let mut base_consumed = 0;
        let mut is_self_trade = false;
        let summary = order_book.place_order(&params, |entry| {
            is_self_trade = is_self_trade || entry.details.owner == params.details.owner;

            let base_amount = entry.size as i128;
            let quote_amount = quote_amount(entry.price, entry.size);

            base_consumed += base_amount;
            quote_consumed += quote_amount;

            match entry.id.side() {
                OrderbookSide::Bid => {
                    base.transfer(
                        &env.current_contract_address(),
                        &entry.details.owner,
                        &base_amount,
                    );

                    quote.transfer(
                        &env.current_contract_address(),
                        &params.details.owner,
                        &quote_amount,
                    );
                }

                OrderbookSide::Ask => {
                    quote.transfer(
                        &env.current_contract_address(),
                        &entry.details.owner,
                        &quote_amount,
                    );

                    base.transfer(
                        &env.current_contract_address(),
                        &params.details.owner,
                        &base_amount,
                    );
                }
            }

            // Consume the maker side events too, since we already transferred their tokens
            //
            // Ideally the events would be consumed separately to avoid conflicts in tx footprints

            let mut orders_to_consume = Map::new(&env);
            orders_to_consume.set(entry.id.clone(), 1);

            order_book.consume_events(orders_to_consume);
        });

        if is_self_trade {
            return Err(DexMarketError::CannotSelfTrade);
        }

        // return unnecessary tokens
        match params.side {
            OrderbookSide::Bid => {
                let return_token_amount = quote_offer_amount
                    - quote_consumed
                    - quote_amount(params.price, summary.posted_size);

                quote.transfer(
                    &env.current_contract_address(),
                    &params.details.owner,
                    &return_token_amount,
                );
            }

            OrderbookSide::Ask => {
                let return_token_amount =
                    (params.size - summary.posted_size) as i128 - base_consumed;

                base.transfer(
                    &env.current_contract_address(),
                    &params.details.owner,
                    &return_token_amount,
                );
            }
        }

        Ok(summary.posted_id)
    }

    /// Cancel a previously placed order
    fn cancel_order(env: Env, order: OrderId) {
        use orderbook::OrderbookSide;

        let order_book = order_book_state(&env);
        let order_detail = order_book.get_order(&order);

        if let Some(order_detail) = order_detail {
            order_detail.details.owner.require_auth();

            let market_info: DexMarketInfo = env.storage().instance().get(&MARKET_INFO).unwrap();
            let base = token::Client::new(&env, &market_info.base_token);
            let quote = token::Client::new(&env, &market_info.quote_token);

            match order_detail.id.side() {
                OrderbookSide::Ask => {
                    base.transfer(
                        &env.current_contract_address(),
                        &order_detail.details.owner,
                        &(order_detail.size as i128),
                    );
                }

                OrderbookSide::Bid => {
                    let token_amount = quote_amount(order_detail.price, order_detail.size);

                    quote.transfer(
                        &env.current_contract_address(),
                        &order_detail.details.owner,
                        &token_amount,
                    );
                }
            }

            order_book.cancel_order(&order);
        }
    }
}

fn order_book_state(env: &Env) -> OrderBook<OrderDetail> {
    OrderBook::open(&env, 0xF1A0)
}

#[contracttype]
struct OrderDetail {
    owner: Address,
}

const MARKET_INFO: Symbol = symbol_short!("MARKETINF");

fn quote_amount(price: u64, base_amount: u128) -> i128 {
    let price = U96F32::from_bits(price as u128);
    let token_amount = price * U96F32::from_num(base_amount);

    token_amount.to_num()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv {
        env: Env,
        base_token: Address,
        quote_token: Address,
        users: std::vec::Vec<Address>,
        market: Address,
    }

    impl TestEnv {
        fn new() -> Self {
            use soroban_sdk::testutils::Address;

            let env = Env::default();
            let base_token = env.register_contract(None, test_token::Token);
            let quote_token = env.register_contract(None, test_token::Token);
            let market = env.register_contract(None, DexMarketContract);

            let market_client = DexMarketContractClient::new(&env, &market);
            market_client.init(&DexMarketInfo {
                base_token: base_token.clone(),
                quote_token: quote_token.clone(),
                base_min_order_size: 1,
            });

            let users = vec![
                soroban_sdk::Address::random(&env),
                soroban_sdk::Address::random(&env),
            ];

            Self {
                env,
                base_token,
                quote_token,
                market,
                users,
            }
        }

        fn market_client(&self) -> DexMarketContractClient {
            DexMarketContractClient::new(&self.env, &self.market)
        }

        fn base_client(&self) -> test_token::TokenClient {
            test_token::TokenClient::new(&self.env, &self.base_token)
        }

        fn quote_client(&self) -> test_token::TokenClient {
            test_token::TokenClient::new(&self.env, &self.quote_token)
        }
    }

    #[test]
    fn test_simple_swap() {
        let ctx = TestEnv::new();

        let market = ctx.market_client();

        ctx.env.mock_all_auths();

        ctx.base_client().mint(&ctx.users[0], &125);
        ctx.quote_client().mint(&ctx.users[1], &100);

        let _ = market
            .place_order(&OrderParams {
                side: OrderSide::Ask,
                size: 125,
                price: (1 << 32),
                owner: ctx.users[0].clone(),
            })
            .unwrap();

        market.place_order(&OrderParams {
            side: OrderSide::Bid,
            size: 100,
            price: (1 << 32),
            owner: ctx.users[1].clone(),
        });

        let balance_0_quote = ctx.quote_client().balance(&ctx.users[0]);
        let balance_1_base = ctx.base_client().balance(&ctx.users[1]);

        assert_eq!(100, balance_0_quote);
        assert_eq!(100, balance_1_base);
    }

    #[test]
    fn test_price_limit_matching() {
        let ctx = TestEnv::new();

        let market = ctx.market_client();

        ctx.env.mock_all_auths();

        ctx.base_client().mint(&ctx.users[0], &1_000);
        ctx.quote_client().mint(&ctx.users[1], &3_000);

        let _ = market
            .place_order(&OrderParams {
                side: OrderSide::Ask,
                size: 1_000,
                price: (2 << 32),
                owner: ctx.users[0].clone(),
            })
            .unwrap();

        market.place_order(&OrderParams {
            side: OrderSide::Bid,
            size: 1_000,
            price: (3 << 32),
            owner: ctx.users[1].clone(),
        });

        let balance_0_quote = ctx.quote_client().balance(&ctx.users[0]);
        let balance_1_base = ctx.base_client().balance(&ctx.users[1]);

        let balance_0_base = ctx.base_client().balance(&ctx.users[0]);
        let balance_1_quote = ctx.quote_client().balance(&ctx.users[1]);

        assert_eq!(0, balance_0_base);
        assert_eq!(2_000, balance_0_quote);

        assert_eq!(1_000, balance_1_base);
        assert_eq!(1_000, balance_1_quote);
    }

    #[test]
    fn test_multiple_matching() {
        let ctx = TestEnv::new();

        let market = ctx.market_client();

        ctx.env.mock_all_auths();

        ctx.base_client().mint(&ctx.users[0], &1_000);
        ctx.quote_client().mint(&ctx.users[1], &3_000);

        for i in 1..5 {
            let _ = market
                .place_order(&OrderParams {
                    side: OrderSide::Ask,
                    price: (i << 32),
                    size: 100 * i as u128,
                    owner: ctx.users[0].clone(),
                })
                .unwrap();
        }

        market.place_order(&OrderParams {
            side: OrderSide::Bid,
            size: 1_000,
            price: (3 << 32),
            owner: ctx.users[1].clone(),
        });

        let balance_0_quote = ctx.quote_client().balance(&ctx.users[0]);
        let balance_1_base = ctx.base_client().balance(&ctx.users[1]);

        let balance_0_base = ctx.base_client().balance(&ctx.users[0]);
        let balance_1_quote = ctx.quote_client().balance(&ctx.users[1]);

        assert_eq!(0, balance_0_base);
        assert_eq!(600, balance_1_base);

        assert_eq!(1_400, balance_0_quote);
        assert_eq!(4_00, balance_1_quote);
    }
}
