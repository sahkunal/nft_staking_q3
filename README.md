# nft_staking_q3

An Anchor program for staking [Metaplex Core](https://developers.metaplex.com/core) NFTs, built for Turbin3's Solana Builders cohort. Unlike classic Token Metadata staking setups, there's no separate `StakeInfo` PDA — staking state lives directly on the Core Asset and Collection accounts as mpl-core `Attributes` plugins.

## Stack

- Anchor `0.32.1`
- `mpl-core` `0.12.1` (Metaplex Core)
- Rust edition 2021

## How it works

Each NFT is a Metaplex Core Asset belonging to a Collection. Staking state is tracked with plugins instead of a separate account:

- **`Attributes` plugin on the Asset** — a flat key/value list holding:
  - `staked`: `"true"` / `"false"`
  - `staked_at`: unix timestamp, set on stake, used only to gate the freeze period on unstake
  - `last_claimed_at`: unix timestamp, a separate accrual clock so `claim_rewards` and `unstake` don't double-pay each other
- **`FreezeDelegate` plugin on the Asset** — freezes the NFT in place while staked (owner keeps it, can't transfer it). Added on stake with its authority delegated to a program PDA, removed entirely on unstake.
- **`BurnDelegate` plugin on the Asset** — lets the program's PDA burn the NFT on the owner's behalf via `burn_staked_nft`, even while frozen. Same add/remove lifecycle as `FreezeDelegate`.
- **`Attributes` plugin on the Collection** — holds a single `total_staked` counter, kept in sync by every stake/unstake/burn through one shared helper (`utils::adjust_total_staked`) so it can't drift.

A single PDA (seeds `["update_authority", collection]`) is the Collection's `update_authority` and holds the delegated `FreezeDelegate`/`BurnDelegate` authority for every Asset in it. It's the signer for all later freeze/thaw/remove/burn/attribute-update operations — but **not** for the initial `AddPlugin` calls that create `FreezeDelegate`/`BurnDelegate` in the first place: those are Owner-Managed plugins, and mpl-core requires the asset owner's own signature to add them, regardless of who the delegated authority ends up being afterward.

## Accounts / PDAs

| PDA | Seeds |
|---|---|
| `config` | `["config", collection]` |
| `update_authority` | `["update_authority", collection]` |
| `rewards_mint` | `["rewards_mint", config]` |

`Config` stores `rewards_bps` (reward rate in basis points per day), `freeze_period` (minimum days before `unstake` is allowed), and the bumps for itself and the rewards mint.

## Instructions

| Instruction | What it does |
|---|---|
| `initialize` | Creates `Config` and the SPL rewards mint for a collection. |
| `create_collection` | Creates the Core Collection, with the PDA as its update authority. |
| `mint_asset` | Mints a Core Asset into the collection, owned by the caller. |
| `stake` | Adds `Attributes`, freezes the asset (`FreezeDelegate`), and delegates burn rights (`BurnDelegate`) to the PDA. Increments `total_staked`. |
| `unstake` | Pays out unclaimed rewards since `last_claimed_at`, thaws and removes `FreezeDelegate`/`BurnDelegate` entirely (so the asset can be staked again later), resets the Attributes. Gated by `freeze_period` days since `staked_at`. Decrements `total_staked`. |
| `claim_rewards` | Mints rewards accrued since `last_claimed_at` without unstaking; advances the accrual clock by whole days paid for. No-ops (no mint, no state change) if less than a day has accrued. Not gated by `freeze_period`. |
| `burn_staked_nft` | Permanently burns a staked NFT for its unclaimed rewards plus a flat `BURN_BONUS_TOKENS` bonus. Thaws first (required — a frozen asset rejects `Burn` even with `BurnDelegate` approval), then burns via the PDA's delegated authority. Decrements `total_staked`. Not gated by `freeze_period`. |

Reward math: `days_elapsed * rewards_bps * 10^decimals / 10000`, using `checked_*` arithmetic throughout to avoid silent overflow.

## Project structure

```
programs/nft_staking_q3/src/
├── lib.rs                      # instruction entrypoints
├── constants.rs                 # seeds, timing, attribute keys
├── error.rs                     # ErrorCode enum
├── utils.rs                     # shared total_staked counter logic
├── state/
│   └── config.rs                 # Config account
└── instructions/
    ├── initialize.rs
    ├── create_collections.rs
    ├── mint_asset.rs
    ├── stake.rs
    ├── unstake.rs
    ├── claim_rewards.rs
    └── burn_staked_nft.rs
tests/
└── nft_staking_q3.ts             # full integration test suite (Anchor + Umi)
```

## Setup

```bash
yarn install
anchor build
```

## Testing

```bash
anchor test
```

This clones the live mpl-core program (`CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d`) onto the ephemeral localnet validator via `Anchor.toml`'s `[test.validator]` config, since mpl-core isn't built by this workspace. `[test] startup_wait = 30000` gives the validator enough time to finish that clone plus normal startup before Anchor's readiness check gives up — the default 5s window is often too tight once a mainnet clone is involved, especially over WSL2.

The test suite covers the full lifecycle: create collection → initialize → mint → stake (×3, checking the attribute fix and `total_staked` increments) → double-stake rejection → no-op claim → full unstake (thaw, plugin removal, counter decrement) → re-stake to prove the plugins were actually removed, not just thawed → burn (bonus payout, counter decrement, asset no longer fetchable as a live mpl-core Asset) → a couple of negative-path checks (`AssetNotStaked`, `InvalidOwner`).

Note on `freeze_period`: the test suite initializes it at `0` so `unstake` doesn't require real wall-clock days to pass. That means day-based reward accrual itself isn't exercised with a nonzero elapsed period in these tests — only its zero-day no-op path and the flat burn bonus are. Testing real multi-day accrual would need a clock-warping harness (e.g. `solana-bankrun`, or the `litesvm` dev-dependency already in `Cargo.toml`) rather than `solana-test-validator`.
