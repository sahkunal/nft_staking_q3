use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("invalid asset owner")]
    InvalidOwner,
    #[msg("invalid input authority")]
    InvalidUpdateAuthority,
    #[msg("assset already staked")]
    AlreadyStaked,
    #[msg("asset not staked")]
    AssetNotStaked,
    #[msg("invalid timestamp")]
    InvalidTimestamp,
    #[msg("freeze period not elapsed")]
    FreezePeriodNotElapsed,
    #[msg("invalid rewards bps")]
    InvalidRewardsBps,
    #[msg("invalid total staked counter")]
    InvalidTotalStaked,
}