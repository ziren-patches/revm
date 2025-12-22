//! Goat params
use alloy_primitives::{address, Address};

pub const GOAT_CHAIN_ID: u64 = 2345;
pub const GOAT_TESTNET_CHAIN_ID: u64 = 48816;
pub const GOAT_HEADER_EXTRA_LENGTH: usize = 33;
pub const GOAT_TX_LIMIT_PER_BLOCK: usize = 128;
pub const GOAT_TX_GAS_LIMIT: u64 = 30_000_000; // the goat tx gas limit, it's the same with eth system tx

pub const GOAT_RELAYER_EXECUTOR: Address = address!("0xbc10000000000000000000000000000000001000");
pub const GOAT_LOCKING_EXECUTOR: Address = address!("0xbc10000000000000000000000000000000001001");

pub const GOAT_FOUNDDATION_CONTRACT: Address =
    address!("0xbc10000000000000000000000000000000000002");
pub const GOAT_LOCKING_CONTRACT: Address = address!("0xbc10000000000000000000000000000000000004");
