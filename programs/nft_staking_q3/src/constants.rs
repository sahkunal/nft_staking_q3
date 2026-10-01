use anchor_lang::prelude::*;

#[constant]
pub const SEED: &str = "anchor";
pub const SECONDS_PER_DAY: i64 = 86400;
pub const BURN_BONUS_TOKENS: u64 = 1000;
pub const ATTR_STAKED: &str = "staked";
pub const ATTR_STAKED_AT: &str = "staked_at";
pub const ATTR_LAST_CLAIMED_AT: &str = "last_claimed_at";
pub const ATTR_TOTAL_STAKED: &str = "total_staked";