use anchor_lang::prelude::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    instructions::{AddPluginV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{
        Attribute, Attributes, BurnDelegate, FreezeDelegate, Plugin, PluginAuthority, PluginType,
        UpdateAuthority,
    },
    fetch_plugin,
};
use crate::constants::{ATTR_LAST_CLAIMED_AT, ATTR_STAKED, ATTR_STAKED_AT};
use crate::state::Config;
use crate::error::ErrorCode;
use crate::utils::adjust_total_staked;

#[derive(Accounts)]
pub struct Stake<'info> {
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
        has_one = update_authority @ ErrorCode::InvalidUpdateAuthority,
    )]
    pub collection: Account<'info, BaseCollectionV1>,
    /// CHECK: PDA that acts as the update authority for the collection and its assets.
    #[account(
        seeds = [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: constrained to the known mpl-core program id.
    #[account(address = MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<Stake>) -> Result<()> {
    let attributes_fetched: Option<Attributes> = fetch_plugin::<BaseAssetV1, Attributes>(
        &ctx.accounts.asset.to_account_info(),
        PluginType::Attributes,
    )
    .ok()
    .map(|(_, attrs, _)| attrs);

    let mut attributes_list: Vec<Attribute> = Vec::new();

    if let Some(attributes) = &attributes_fetched {
        for attribute in &attributes.attribute_list {
            if attribute.key == ATTR_STAKED {
                require!(attribute.value == "false", ErrorCode::AlreadyStaked);
            } else if attribute.key != ATTR_STAKED_AT && attribute.key != ATTR_LAST_CLAIMED_AT {
                attributes_list.push(attribute.clone());
            }
        }
    }

    let now = Clock::get()?.unix_timestamp.to_string();

    // NOTE: the original version of this instruction had `staked` and
    // `staked_at` swapped (key/value reversed), which meant `unstake` could
    // never successfully read them back. Fixed here: `staked` is the
    // "true"/"false" flag, `staked_at` and `last_claimed_at` are timestamps.
    attributes_list.push(Attribute {
        key: ATTR_STAKED.to_string(),
        value: "true".to_string(),
    });
    attributes_list.push(Attribute {
        key: ATTR_STAKED_AT.to_string(),
        value: now.clone(),
    });
    // Separate accrual-window timestamp so `claim_rewards` can advance it
    // independently without disturbing the freeze-period check above.
    attributes_list.push(Attribute {
        key: ATTR_LAST_CLAIMED_AT.to_string(),
        value: now,
    });

    let collection_key = ctx.accounts.collection.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        b"update_authority",
        collection_key.as_ref(),
        &[ctx.bumps.update_authority],
    ]];

    // If the Attributes plugin does not exist, we add it
    if attributes_fetched.is_none() {
        AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
            .asset(&ctx.accounts.asset.to_account_info())
            .collection(Some(&ctx.accounts.collection.to_account_info()))
            .payer(&ctx.accounts.owner.to_account_info())
            .authority(Some(&ctx.accounts.update_authority.to_account_info()))
            .system_program(&ctx.accounts.system_program.to_account_info())
            .plugin(Plugin::Attributes(Attributes {
                attribute_list: attributes_list,
            }))
            .init_authority(PluginAuthority::UpdateAuthority)
            .invoke_signed(signer_seeds)?;
    }
    // If the Attributes plugin exists, we update it
    else {
        UpdatePluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
            .asset(&ctx.accounts.asset.to_account_info())
            .collection(Some(&ctx.accounts.collection.to_account_info()))
            .payer(&ctx.accounts.owner.to_account_info())
            .authority(Some(&ctx.accounts.update_authority.to_account_info()))
            .system_program(&ctx.accounts.system_program.to_account_info())
            .plugin(Plugin::Attributes(Attributes {
                attribute_list: attributes_list,
            }))
            .invoke_signed(signer_seeds)?;
    }

    // Freeze the asset in place (owner keeps it, but it can't be transferred).
    // FreezeDelegate is Owner-Managed: mpl-core requires the OWNER's
    // signature to add it, regardless of who signs the surrounding tx — the
    // PDA only becomes the plugin's authority *after* this succeeds, via
    // init_authority below. Using the PDA as `authority` here (as I
    // originally had it) fails with mpl-core's NoApprovals error (0x1a).
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.owner.to_account_info()))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::FreezeDelegate(FreezeDelegate { frozen: true }))
        .init_authority(PluginAuthority::UpdateAuthority)
        .invoke()?;

    // Delegate burn authority to the update_authority PDA so
    // `burn_staked_nft` can burn on the owner's behalf later, even though
    // the asset will be frozen. BurnDelegate is also Owner-Managed, so this
    // add must be authorized by the owner, same reasoning as above.
    AddPluginV1CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
        .asset(&ctx.accounts.asset.to_account_info())
        .collection(Some(&ctx.accounts.collection.to_account_info()))
        .payer(&ctx.accounts.owner.to_account_info())
        .authority(Some(&ctx.accounts.owner.to_account_info()))
        .system_program(&ctx.accounts.system_program.to_account_info())
        .plugin(Plugin::BurnDelegate(BurnDelegate {}))
        .init_authority(PluginAuthority::UpdateAuthority)
        .invoke()?;

    // Task 3: bump the collection-level total_staked counter.
    adjust_total_staked(
        &ctx.accounts.mpl_core_program.to_account_info(),
        &ctx.accounts.collection.to_account_info(),
        &ctx.accounts.owner.to_account_info(),
        &ctx.accounts.update_authority.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &[
            b"update_authority",
            collection_key.as_ref(),
            &[ctx.bumps.update_authority],
        ],
        1,
    )?;

    Ok(())
}