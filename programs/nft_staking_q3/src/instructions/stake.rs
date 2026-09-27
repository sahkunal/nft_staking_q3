use anchor_lang::prelude ::*;
use mpl_core::{
    ID as MPL_CORE_ID,
    accounts::{BaseAssetV1, BaseCollectionV1},
    instructions::{AddPluginV1CpiBuilder, UpdatePluginV1CpiBuilder},
    types::{UpdateAuthority, Attribute, Attributes, Plugin, PluginAuthority, PluginType, FreezeDelegate},
    fetch_plugin,
};
use crate ::state::Config;
use crate:: error::ErrorCode;

#[derive(Accounts)]

pub struct Stake<'info>{
    #[account(mut)]
    pub owner:Signer<'info>,
    #[account(
        seeds=[b"config", collection.key().as_ref()],
        bump = config.bumps,
    )]
    pub config: Account<'info,Config>,
    #[account(
        mut,
        has_one= owner @ ErrorCode::InvalidOwner,
        constraint= asset.update_authority== UpdateAuthority:: Collection(collection.key()) @ ErrorCode:: InvalidUpdateAuthority,
    )]
    pub asset:Account<'info, BaseAssetV1>,
    #[account(
        mut,
        has_one= update_authority @ ErrorCode:: InvalidUpdateAuthority,
    )]
    pub collection:Account<'info, BaseCollectionV1>,
    ///CHECK
    #[account(
        seeds= [b"update_authority", collection.key().as_ref()],
        bump,
    )]
    pub update_authority: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK
    #[account(address= MPL_CORE_ID)]
    pub mpl_core_program: UncheckedAccount<'info>,
}

    pub fn handler(ctx: Context<Stake>)-> Result<()>{
        let attributes_fetched: Option<Attributes>= fetch_plugin::<BaseAssetV1, Attributes>(
            &ctx.accounts.asset.to_account_info(),
            PluginType::Attributes,
        )
        .ok()
        .map(|(_,attrs,_)|attrs);
    let mut attributes_list: Vec<Attribute>= Vec::new();

    if let Some(attributes)= &attributes_fetched{
        for attribute in &attributes.attribute_list{
            if attribute.key== "staked"{
                require!(attribute.value== "false", ErrorCode:: AlreadyStaked);
        }
        else if attribute.key!="staked_at"{
            attributes_list.push(attribute.clone());
        }
    }
    }
    //add the staking attributes
    attributes_list.push(Attribute { 
        key: "staked_at".to_string(),
        value:"true".to_string(),
});
     attributes_list.push(Attribute { 
        key: "staked".to_string(),
        value:Clock::get()?.unix_timestamp.to_string(),
     });

     let collection_key = ctx.accounts.collection.key();
     let signer_seeds:&[&[&[u8]]]=&[&[
        b"update_authority",
        collection_key.as_ref(),
        &[ctx.bumps.update_authority],
     ]] ;

    // If the Attributes plugin does not exist, we add it
if attributes_fetched.is_none() {

    AddPluginV1CpiBuilder::new(
        &ctx.accounts.mpl_core_program.to_account_info()
    )
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::Attributes( Attributes {
            attribute_list: attributes_list
        }
    ))
    .init_authority(PluginAuthority::UpdateAuthority)
    .invoke_signed(signer_seeds)?;
}

// If the Attributes plugin exists, we update it
else {

    UpdatePluginV1CpiBuilder::new(
        &ctx.accounts.mpl_core_program.to_account_info()
    )
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::Attributes(
        Attributes {
            attribute_list: attributes_list
        }
    ))
    .invoke_signed(signer_seeds)?;

}


AddPluginV1CpiBuilder::new(
        &ctx.accounts.mpl_core_program.to_account_info()
    )
    .asset(&ctx.accounts.asset.to_account_info())
    .collection(Some(&ctx.accounts.collection.to_account_info()))
    .payer(&ctx.accounts.owner.to_account_info())
    .authority(Some(&ctx.accounts.update_authority.to_account_info()))
    .system_program(&ctx.accounts.system_program.to_account_info())
    .plugin(Plugin::FreezeDelegate(FreezeDelegate{frozen: true}))
    .init_authority(PluginAuthority::UpdateAuthority)
    .invoke_signed(signer_seeds)?;
    
    Ok(())
}
