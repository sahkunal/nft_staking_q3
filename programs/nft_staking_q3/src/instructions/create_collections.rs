use anchor_lang::prelude::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    instructions::CreateCollectionV2CpiBuilder,
};
#[derive(Accounts)]

pub struct CreateCollection<'info>{
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut)]
    pub collection: Signer<'info>,
/// CHECK:
    #[account(
        seeds= [b"update_authority", collection.key().as_ref()],
        bump,
    )] 

    pub update_authority: UncheckedAccount<'info>,
    pub system_account: Program<'info, System>,
/// CHECK: 
    #[account(address= MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}

pub fn handler(ctx: Context<CreateCollection>, name: String, uri: String)-> Result<()>{
    let collection_key= ctx.accounts.collection.key();
    let signer_seeds: &[&[u8]]= &[
        b"update_authority",
        collection_key.as_ref(),
        &[ctx.bumps.update_authority],
    ];

    CreateCollectionV2CpiBuilder::new(&ctx.accounts.mpl_core_program.to_account_info())
    .collection(&ctx.accounts.collection.to_account_info())
    .payer(&ctx.accounts.payer.to_account_info())
    .update_authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_account.to_account_info())
    .name(name)
    .uri(uri)
    .invoke_signed(&[signer_seeds])?;

    Ok(())
}