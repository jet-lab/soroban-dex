use core::fmt::Debug;

use soroban_sdk::{contracttype, Bytes, BytesN, Env};

use crate::OrderSide;

/// An identifier for an order in the book
///
/// This is also the key for the order in the contract storage
///
/// Structure:
///     - 2 bytes: prefix (for contract storage namespacing)
///     - 1 byte: order side
///     - 1 byte: reserved
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

/// An event indicating some action needs to be completed
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrderEvent {
    /// The order has been partially filled
    Fill(u128),
}
