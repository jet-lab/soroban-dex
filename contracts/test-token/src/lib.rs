#![no_std]

use soroban_sdk::{contract, contractimpl, token::Interface, Address, Env, String};
use soroban_token_sdk::TokenUtils;

#[contract]
pub struct Token;

#[contractimpl]
impl Token {
    pub fn mint(e: Env, to: Address, amount: i128) {
        let balance = Self::balance(e.clone(), to.clone());
        e.storage().persistent().set(&to, &(balance + amount));

        TokenUtils::new(&e).events().mint(to.clone(), to, amount);
    }
}

#[contractimpl]
impl Interface for Token {
    fn allowance(_e: Env, _from: Address, _spender: Address) -> i128 {
        todo!()
    }

    fn approve(_e: Env, _from: Address, _spender: Address, _amount: i128, _expiration_ledger: u32) {
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

        if from_balance < amount {
            panic!(
                "insufficient balance, has {} but needs {}",
                from_balance, amount
            );
        }

        e.storage()
            .persistent()
            .set(&from, &(from_balance - amount));
        e.storage().persistent().set(&to, &(to_balance + amount));

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
