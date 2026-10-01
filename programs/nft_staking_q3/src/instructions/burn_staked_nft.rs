use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, MintToChecked, TokenAccount, TokenInterface, mint_to_checked},
};
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    types::{Attributes, FreezeDelegate, Plugin, PluginType, UpdateAuthority},
    instructions::{BurnV1CpiBuilder, UpdatePluginV1CpiBuilder},
    fetch_plugin,
};
use crate::constants::{ATTR_LAST_CLAIMED_AT, ATTR_STAKED, BURN_BONUS_TOKENS, SECONDS_PER_DAY};
use crate::Config;
use crate::error::ErrorCode;
use crate::utils::adjust_total_staked;

#[derive(Accounts)]
pub struct BurnStakedNft<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        seeds = [b"config", collection.key().as_ref()],
        bump = config.bumps,
    )]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        has_one = owner @ ErrorCode::InvalidOwner,
        constraint = asset.update_authority == UpdateAuthority::Collection(collection.key()) @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub asset: Account<'info, BaseAssetV1>,
    #[account(
        mut,
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK:
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"rewards_mint", config.key().as_ref()],
        bump = config.rewards_bump,
    )]
    pub rewards_mint: InterfaceAccount<'info, Mint>,
    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = rewards_mint,
        associated_token::authority = owner,
    )]
    pub user_rewards_ata: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    /// CHECK:
    #[account(address = Pubkey::from(MPL_CORE_ID.to_bytes()))]
    pub mpl_core_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<BurnStakedNft>) -> Result<()> {
    let attributes_fetched: Option<Attributes> = fetch_plugin::<BaseAssetV1, Attributes>(
        &ctx.accounts.asset.to_account_info(),
        PluginType::Attributes,
    )
    .ok()
    .map(|(_, attrs, _)| attrs);

    require!(attributes_fetched.is_some(), ErrorCode::AssetNotStaked);
    let attributes = attributes_fetched.unwrap();

    let current_timestamp = Clock::get()?.unix_timestamp;
    let mut last_claimed_timestamp = 0i64;

    for attribute in &attributes.attribute_list {
        if attribute.key == ATTR_STAKED {
            require!(attribute.value == "true", ErrorCode::AssetNotStaked);
        } else if attribute.key == ATTR_LAST_CLAIMED_AT {
            last_claimed_timestamp = attribute
                .value
                .parse::<i64>()
                .map_err(|_| ErrorCode::InvalidTimestamp)?;
        }
    }

    // No freeze-period gate: burning is allowed at any time while staked.
    let unclaimed_days = current_timestamp
        .checked_sub(last_claimed_timestamp)
        .ok_or(ErrorCode::InvalidTimestamp)?
        .checked_div(SECONDS_PER_DAY)
        .ok_or(ErrorCode::InvalidTimestamp)?
        .max(0);

    let unclaimed_amount: u64 = (unclaimed_days as u64)
        .checked_mul(ctx.accounts.config.rewards_bps as u64)
        .ok_or(ErrorCode::InvalidRewardsBps)?
        .checked_mul(10u64.pow(ctx.accounts.rewards_mint.decimals as u32))
        .ok_or(ErrorCode::InvalidRewardsBps)?
        .checked_div(10000u64)
        .ok_or(ErrorCode::InvalidRewardsBps)?;

    let bonus_amount: u64 = BURN_BONUS_TOKENS
        .checked_mul(10u64.pow(ctx.accounts.rewards_mint.decimals as u32))
        .ok_or(ErrorCode::InvalidRewardsBps)?;

    let total_amount = unclaimed_amount
        .checked_add(bonus_amount)
        .ok_or(ErrorCode::InvalidRewardsBps)?;

    let collection_key = ctx.accounts.collection.key();
    let signer_seeds: &[&[u8]] = &[
        b"update_authority",
        collection_key.as_ref(),
        &[ctx.bumps.update_authority],
    ];

    // FreezeDelegate rejects Burn while frozen == true, and BurnDelegate's
    // approval can't override that rejection — so thaw first. The PDA holds
    // FreezeDelegate's authority (delegated at stake time), so it can do
    // this on its own, without the owner co-signing anything extra here.
    UpdatePluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.update_authority.to_account_info()))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: false }))
        .invoke_signed(&[signer_seeds])?;

    // Burn via the BurnDelegate authority we delegated to the PDA at stake
    // time — the owner doesn't need to co-sign the burn itself.
    BurnV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.update_authority.to_account_info()))
        .system_program(Some(&ctx.accounts.system_program.to_account_info()))
        .invoke_signed(&[signer_seeds])?;

    let config_seeds: &[&[u8]; 3] = &[
        b"config",
        collection_key.as_ref(),
        &[ctx.accounts.config.bumps],
    ];
    let config_signer_seeds: &[&[&[u8]]] = &[config_seeds];

    mint_to_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintToChecked {
                mint: ctx.accounts.rewards_mint.to_account_info(),
                to: ctx.accounts.user_rewards_ata.to_account_info(),
                authority: ctx.accounts.config.to_account_info(),
            },
            config_signer_seeds,
        ),
        total_amount,
        ctx.accounts.rewards_mint.decimals,
    )?;

    // Task 3: the burned NFT is no longer staked either.
    adjust_total_staked(
        &ctx.accounts.mpl_core_program.to_account_info(),
        &ctx.accounts.collection.to_account_info(),
        &ctx.accounts.owner.to_account_info(),
        &ctx.accounts.update_authority.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        signer_seeds,
        -1,
    )?;

    Ok(())
}