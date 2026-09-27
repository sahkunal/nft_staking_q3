use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]

pub struct Config{
    pub rewards_bps:u16, //rewards percentage in basis points
    pub freeze_period:u16, // minimun freeze period in days
    pub rewards_bump:u8, // bumps for the rewards mint account
    pub bumps:u8, //bumps for the config account
}