#![cfg_attr(not(test), no_std)]

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

        params.details.owner.require_auth();

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

            let price = U96F32::from_bits(entry.price as u128);
            let base_amount = entry.size as i128;
            let quote_amount = (price * U96F32::from_num(entry.size)).to_num();

            match entry.id.side() {
                OrderSide::Bid => {
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

                OrderSide::Ask => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{token::Interface, String};
    use soroban_token_sdk::TokenUtils;

    #[contract]
    struct MockToken;

    #[contractimpl]
    impl soroban_sdk::token::Interface for MockToken {
        fn allowance(_e: Env, _from: Address, _spender: Address) -> i128 {
            todo!()
        }

        fn approve(
            _e: Env,
            _from: Address,
            _spender: Address,
            _amount: i128,
            _expiration_ledger: u32,
        ) {
            todo!()
        }

        fn balance(e: Env, id: Address) -> i128 {
            e.storage()
                .persistent()
                .get::<Address, i128>(&id)
                .unwrap_or(0)
        }

        fn spendable_balance(e: Env, id: Address) -> i128 {
            Self::balance(e, id)
        }

        fn transfer(e: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();

            let from_balance = Self::balance(e.clone(), from.clone());
            let to_balance = Self::balance(e.clone(), to.clone());

            e.storage()
                .persistent()
                .set(&from, &(from_balance - amount));
            e.storage().persistent().set(&from, &(to_balance + amount));

            TokenUtils::new(&e).events().transfer(from, to, amount);
        }

        fn transfer_from(_e: Env, _spender: Address, _from: Address, _to: Address, _amount: i128) {
            todo!()
        }

        fn burn(_e: Env, _from: Address, _amount: i128) {
            todo!()
        }

        fn burn_from(_e: Env, _spender: Address, _from: Address, _amount: i128) {
            todo!()
        }

        fn decimals(_e: Env) -> u32 {
            0
        }

        fn name(e: Env) -> String {
            String::from_slice(&e, "MockToken")
        }

        fn symbol(e: Env) -> String {
            String::from_slice(&e, "MockToken")
        }
    }

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
            let base_token = env.register_contract(None, MockToken);
            let quote_token = env.register_contract(None, MockToken);
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

        fn base_client(&self) -> MockTokenClient {
            MockTokenClient::new(&self.env, &self.quote_token)
        }

        fn quote_client(&self) -> MockTokenClient {
            MockTokenClient::new(&self.env, &self.base_token)
        }
    }

    #[test]
    fn test_simple_swap() {
        let ctx = TestEnv::new();

        let market = ctx.market_client();

        ctx.env.mock_all_auths();

        let _ = market
            .place_order(&OrderParams {
                side: OrderSide::Ask,
                size: 100,
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
}
