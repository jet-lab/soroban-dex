use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short,
    token::{Interface, StellarAssetInterface},
    Address, Bytes, Env, Map, String, Symbol,
};

#[contract]
pub struct DexToken;

#[contractimpl]
impl DexToken {
    pub fn initialize(env: Env, admin: Address, decimals: u32, name: String, symbol: String) {
        // if has_administrator(&e) {
        //     panic!("already initialized")
        // }
        // write_administrator(&e, &admin);
        // if decimal > u8::MAX.into() {
        //     panic!("Decimal must fit in a u8");
        // }

        // write_metadata(
        //     &e,
        //     TokenMetadata {
        //         decimal,
        //         name,
        //         symbol,
        //     },
        // )
    }

    fn mint(env: Env, to: Address, amount: i128) {
        todo!()
    }
}

#[contractimpl]
impl Interface for DexToken {
    fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        todo!()
    }

    fn approve(env: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
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
        todo!()
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
        String::from_slice(&e, "DexToken")
    }

    fn symbol(e: Env) -> String {
        String::from_slice(&e, "DexToken")
    }
}
