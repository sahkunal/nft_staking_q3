use anchor_lang::prelude::*;
use mpl_core::{
    accounts::BaseCollectionV1,
    fetch_plugin,
    instructions::{AddCollectionPluginV1CpiBuilder, UpdateCollectionPluginV1CpiBuilder},
    types::{Attribute, Attributes, Plugin, PluginAuthority, PluginType},
};

use crate::constants::ATTR_TOTAL_STAKED;
use crate::error::ErrorCode;

/// Adds `delta` (positive or negative) to the `total_staked` Attribute on the
/// Collection account, creating the Attributes plugin on first use.
///
/// Shared by `stake` (+1), `unstake` (-1) and `burn_staked_nft` (-1), so the
/// counter can never drift out of sync with any one instruction's logic.
#[allow(clippy::too_many_arguments)]
pub fn adjust_total_staked<'info>(
    mpl_core_program: &AccountInfo<'info>,
    collection: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    update_authority: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    signer_seeds: &[&[u8]],
    delta: i64,
) -> Result<()> {
    let attributes_fetched: Option<Attributes> =
        fetch_plugin::<BaseCollectionV1, Attributes>(collection, PluginType::Attributes)
            .ok()
            .map(|(_, attrs, _)| attrs);

    let mut attributes_list: Vec<Attribute> = Vec::new();
    let mut current_total: i64 = 0;
    let mut found = false;

    if let Some(attributes) = &attributes_fetched {
        for attribute in &attributes.attribute_list {
            if attribute.key == ATTR_TOTAL_STAKED {
                current_total = attribute
                    .value
                    .parse::<i64>()
                    .map_err(|_| ErrorCode::InvalidTotalStaked)?;
                found = true;
            } else {
                attributes_list.push(attribute.clone());
            }
        }
    }
    // Belt-and-braces: if the plugin existed but somehow never had the key,
    // current_total stays 0, which is the correct starting point.
    let _ = found;

    let new_total = current_total
        .checked_add(delta)
        .ok_or(ErrorCode::InvalidTotalStaked)?;
    require!(new_total >= 0, ErrorCode::InvalidTotalStaked);

    attributes_list.push(Attribute {
        key: ATTR_TOTAL_STAKED.to_string(),
        value: new_total.to_string(),
    });

    let signers: &[&[&[u8]]] = &[signer_seeds];

    if attributes_fetched.is_none() {
        AddCollectionPluginV1CpiBuilder::new(mpl_core_program)
            .collection(collection)
            .payer(payer)
            .authority(Some(update_authority))
            .system_program(system_program)
            .plugin(Plugin::Attributes(Attributes {
                attribute_list: attributes_list,
            }))
            .init_authority(PluginAuthority::UpdateAuthority)
            .invoke_signed(signers)?;
    } else {
        UpdateCollectionPluginV1CpiBuilder::new(mpl_core_program)
            .collection(collection)
            .payer(payer)
            .authority(Some(update_authority))
            .system_program(system_program)
            .plugin(Plugin::Attributes(Attributes {
                attribute_list: attributes_list,
            }))
            .invoke_signed(signers)?;
    }

    Ok(())
}