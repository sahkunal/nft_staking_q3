#![allow(unexpected_cfgs, deprecated, ambiguous_glob_reexports)]
pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;
pub mod utils;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;
declare_id!("CX4TYHcukXuv1kM343nYvSGpprQJVZndj1ktwkUZiffQ");

#[program]
pub mod nft_staking_k {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, rewards_bps:u16, freeze_period:u16) -> Result<()> {
       initialize::handler(ctx, rewards_bps, freeze_period)
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>)->Result<()> {
        claim_rewards::handler(ctx)
    }
    pub fn create_collection(ctx: Context<CreateCollection>, name: String, uri:String)-> Result<()>{
        create_collections::handler(ctx, name, uri)
    }
    pub fn mint_asset(ctx: Context<MintAsset>, name:String, uri:String)-> Result<()>{
        mint_asset::handler(ctx, name, uri) 
    }
   pub fn stake(ctx: Context<Stake>)-> Result<()>{
       stake::handler(ctx)
    }
     pub fn unstake(ctx: Context<Unstake>)-> Result<()>{
       unstake::handler(ctx)
}
}