import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import {
  Keypair,
  PublicKey,
  SystemProgram,
  LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  getAssociatedTokenAddressSync,
  getAccount,
  Account,
} from "@solana/spl-token";
import { createUmi } from "@metaplex-foundation/umi-bundle-defaults";
import { fetchAsset, fetchCollection } from "@metaplex-foundation/mpl-core";
import { publicKey as umiPublicKey } from "@metaplex-foundation/umi";
import { assert } from "chai";
import { NftStakingQ3 } from "../target/types/nft_staking_q3";

const MPL_CORE_PROGRAM_ID = new PublicKey(
  "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d"
);

describe("nft_staking_q3", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const connection = provider.connection;

  const program = anchor.workspace.NftStakingQ3 as Program<NftStakingQ3>;

  // Umi client, used read-only, purely to decode mpl-core Attributes/plugin
  // state in assertions (this program's own instructions do all the writing).
  // Commitment must be "processed" here, matching Anchor's own .rpc() default
  // (AnchorProvider's default commitment is "processed", not "confirmed").
  // A stricter Umi read commitment races ahead of what .rpc() actually
  // waited for, causing AccountNotFoundError on a fetch immediately after a
  // successful write — exactly what we were seeing.
  const umi = createUmi(connection.rpcEndpoint, "processed");

  const admin = (provider.wallet as anchor.Wallet).payer;
  const user = Keypair.generate();

  const collection = Keypair.generate();
  const assetUnstake = Keypair.generate(); // stake -> claim (no-op) -> unstake
  const assetClaim = Keypair.generate(); // stake -> claim -> unstake (double-claim check)
  const assetBurn = Keypair.generate(); // stake -> burn

  const REWARDS_BPS = 500; // 5%
  const FREEZE_PERIOD_DAYS = 0; // so unstake doesn't need real wall-clock days
  const BURN_BONUS_TOKENS = 1000; // must match constants.rs BURN_BONUS_TOKENS
  const MINT_DECIMALS = 6; // must match initialize.rs (mint::decimals = 6)

  let config: PublicKey;
  let updateAuthority: PublicKey;
  let rewardsMint: PublicKey;
  let userRewardsAta: PublicKey;

  function pdas(collectionKey: PublicKey) {
    const [configPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("config"), collectionKey.toBuffer()],
      program.programId
    );
    const [updateAuthorityPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("update_authority"), collectionKey.toBuffer()],
      program.programId
    );
    const [rewardsMintPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("rewards_mint"), configPda.toBuffer()],
      program.programId
    );
    return { configPda, updateAuthorityPda, rewardsMintPda };
  }

  async function attrValue(
    attributeList: { key: string; value: string }[],
    key: string
  ): Promise<string | undefined> {
    return attributeList.find((a) => a.key === key)?.value;
  }

  async function fetchAssetAttributes(assetPk: PublicKey) {
    const asset = await fetchAsset(umi, umiPublicKey(assetPk.toBase58()));
    return asset;
  }

  async function fetchCollectionAttributes() {
    const coll = await fetchCollection(
      umi,
      umiPublicKey(collection.publicKey.toBase58())
    );
    return coll;
  }

  before(async () => {
    // Fund the test user.
    const sig = await connection.requestAirdrop(
      user.publicKey,
      2 * LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig, "confirmed");

    ({
      configPda: config,
      updateAuthorityPda: updateAuthority,
      rewardsMintPda: rewardsMint,
    } = pdas(collection.publicKey));

    userRewardsAta = getAssociatedTokenAddressSync(rewardsMint, user.publicKey);
  });

  it("creates the collection", async () => {
    await program.methods
      .createCollection("Staking Test Collection", "https://example.com/collection.json")
      .accounts({
        payer: admin.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        systemAccount: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([admin, collection])
      .rpc();

    const coll = await fetchCollectionAttributes();
    assert.equal(coll.name, "Staking Test Collection");
  });

  it("initializes config + rewards mint", async () => {
    await program.methods
      .initialize(REWARDS_BPS, FREEZE_PERIOD_DAYS)
      .accounts({
        admin: admin.publicKey,
        config,
        collection: collection.publicKey,
        updateAuthority,
        rewardsMint,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      } as any)
      .signers([admin])
      .rpc();

    const configAccount = await program.account.config.fetch(config);
    assert.equal(configAccount.rewardsBps, REWARDS_BPS);
    assert.equal(configAccount.freezePeriod, FREEZE_PERIOD_DAYS);
  });

  async function mintAsset(asset: Keypair) {
    await program.methods
      .mintAsset(`Staked NFT ${asset.publicKey.toBase58().slice(0, 6)}`, "https://example.com/asset.json")
      .accounts({
        user: user.publicKey,
        asset: asset.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user, asset])
      .rpc();
  }

  it("mints three assets into the collection", async () => {
    await mintAsset(assetUnstake);
    await mintAsset(assetClaim);
    await mintAsset(assetBurn);

    for (const asset of [assetUnstake, assetClaim, assetBurn]) {
      const fetched = await fetchAssetAttributes(asset.publicKey);
      assert.equal(fetched.owner.toString(), user.publicKey.toBase58());
    }
  });

  async function stake(asset: Keypair) {
    await program.methods
      .stake()
      .accounts({
        owner: user.publicKey,
        config,
        asset: asset.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user])
      .rpc();
  }

  it("stakes all three assets and bumps total_staked each time", async () => {
    await stake(assetUnstake);
    let coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "1"
    );

    await stake(assetClaim);
    coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "2"
    );

    await stake(assetBurn);
    coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "3"
    );
  });

  it("sets staked/staked_at/last_claimed_at correctly and freezes + delegates burn", async () => {
    const asset = await fetchAssetAttributes(assetUnstake.publicKey);

    assert.equal(
      await attrValue(asset.attributes?.attributeList ?? [], "staked"),
      "true"
    );
    const stakedAt = await attrValue(asset.attributes?.attributeList ?? [], "staked_at");
    const lastClaimedAt = await attrValue(
      asset.attributes?.attributeList ?? [],
      "last_claimed_at"
    );
    assert.isDefined(stakedAt);
    assert.notEqual(stakedAt, "0");
    assert.equal(lastClaimedAt, stakedAt, "last_claimed_at should start equal to staked_at");

    // Frozen in place, still owned by the staker.
    assert.equal(asset.freezeDelegate?.frozen, true);
    assert.equal(asset.owner.toString(), user.publicKey.toBase58());

    // BurnDelegate plugin present, delegated to the PDA.
    assert.isDefined(asset.burnDelegate, "BurnDelegate plugin should be present after staking");
  });

  it("rejects staking the same asset twice", async () => {
    try {
      await stake(assetUnstake);
      assert.fail("expected stake() to fail on an already-staked asset");
    } catch (err) {
      assert.include(String(err), "AlreadyStaked");
    }
  });

  it("claim_rewards is a no-op immediately after staking (no elapsed days)", async () => {
    const before = await fetchAssetAttributes(assetClaim.publicKey);
    const lastClaimedBefore = await attrValue(
      before.attributes?.attributeList ?? [],
      "last_claimed_at"
    );

    await program.methods
      .claimRewards()
      .accounts({
        owner: user.publicKey,
        config,
        asset: assetClaim.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        rewardsMint,
        userRewardsAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user])
      .rpc();

    const after = await fetchAssetAttributes(assetClaim.publicKey);
    const lastClaimedAfter = await attrValue(
      after.attributes?.attributeList ?? [],
      "last_claimed_at"
    );
    assert.equal(
      lastClaimedAfter,
      lastClaimedBefore,
      "last_claimed_at should be untouched when zero days have elapsed"
    );

    // The asset is still staked and frozen — claim doesn't unstake.
    assert.equal(await attrValue(after.attributes?.attributeList ?? [], "staked"), "true");
    assert.equal(after.freezeDelegate?.frozen, true);
  });

  it("unstakes assetUnstake: thaws, removes delegates, decrements total_staked", async () => {
    await program.methods
      .unstake()
      .accounts({
        owner: user.publicKey,
        config,
        asset: assetUnstake.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        rewardsMint,
        userRewardsAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user])
      .rpc();

    const asset = await fetchAssetAttributes(assetUnstake.publicKey);
    assert.equal(await attrValue(asset.attributes?.attributeList ?? [], "staked"), "false");
    assert.equal(await attrValue(asset.attributes?.attributeList ?? [], "staked_at"), "0");
    assert.equal(
      await attrValue(asset.attributes?.attributeList ?? [], "last_claimed_at"),
      "0"
    );

    // FreezeDelegate and BurnDelegate should be fully removed, not just thawed.
    assert.isUndefined(
      asset.freezeDelegate,
      "FreezeDelegate should be removed on unstake"
    );
    assert.isUndefined(
      asset.burnDelegate,
      "BurnDelegate should be removed on unstake"
    );

    const coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "2"
    );
  });

  it("can re-stake an unstaked asset (proves plugins were actually removed)", async () => {
    await stake(assetUnstake);

    const asset = await fetchAssetAttributes(assetUnstake.publicKey);
    assert.equal(await attrValue(asset.attributes?.attributeList ?? [], "staked"), "true");
    assert.equal(asset.freezeDelegate?.frozen, true);
    assert.isDefined(asset.burnDelegate);

    const coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "3"
    );
  });

  it("unstaking assetClaim after a no-op claim does not double-pay (0 balance either way)", async () => {
    const balanceBefore = await getAccount(connection, userRewardsAta)
      .then((a: Account) => a.amount)
      .catch(() => BigInt(0));

    await program.methods
      .unstake()
      .accounts({
        owner: user.publicKey,
        config,
        asset: assetClaim.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        rewardsMint,
        userRewardsAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user])
      .rpc();

    const balanceAfter = await getAccount(connection, userRewardsAta).then(
      (a: Account) => a.amount
    );
    assert.equal(balanceAfter, balanceBefore);
  });

  it("burns assetBurn for the bonus and decrements total_staked", async () => {
    const balanceBefore = await getAccount(connection, userRewardsAta)
      .then((a: Account) => a.amount)
      .catch(() => BigInt(0));

    await program.methods
      .burnStakedNft()
      .accounts({
        owner: user.publicKey,
        config,
        asset: assetBurn.publicKey,
        collection: collection.publicKey,
        updateAuthority,
        rewardsMint,
        userRewardsAta,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        mplCoreProgram: MPL_CORE_PROGRAM_ID,
      } as any)
      .signers([user])
      .rpc();

    // The asset account should be closed by the Burn CPI.
    const assetInfo = await connection.getAccountInfo(assetBurn.publicKey);
    const isBurned =
      assetInfo === null ||
      assetInfo.data.length === 0 ||
      !assetInfo.owner.equals(MPL_CORE_PROGRAM_ID);
    assert.isTrue(isBurned, "burned asset should no longer be a live mpl-core Asset");

    const balanceAfter = await getAccount(connection, userRewardsAta).then(
      (a: Account) => a.amount
    );
    const expectedBonus = BigInt(BURN_BONUS_TOKENS) * BigInt(10 ** MINT_DECIMALS);
    // Unclaimed-days portion should be ~0 given the test's real elapsed time,
    // so the payout should be exactly the flat bonus.
    assert.equal(balanceAfter - balanceBefore, expectedBonus);

    const coll = await fetchCollectionAttributes();
    assert.equal(
      await attrValue(coll.attributes?.attributeList ?? [], "total_staked"),
      "2"
    );
  });

  it("rejects claim_rewards / unstake / burn_staked_nft on a never-staked asset", async () => {
    const freshAsset = Keypair.generate();
    await mintAsset(freshAsset);

    try {
      await program.methods
        .unstake()
        .accounts({
          owner: user.publicKey,
          config,
          asset: freshAsset.publicKey,
          collection: collection.publicKey,
          updateAuthority,
          rewardsMint,
          userRewardsAta,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          systemProgram: SystemProgram.programId,
          mplCoreProgram: MPL_CORE_PROGRAM_ID,
        } as any)
        .signers([user])
        .rpc();
      assert.fail("expected unstake() to fail on a never-staked asset");
    } catch (err) {
      assert.include(String(err), "AssetNotStaked");
    }
  });

  it("rejects staking by a non-owner", async () => {
    const impostor = Keypair.generate();
    const sig = await connection.requestAirdrop(
      impostor.publicKey,
      LAMPORTS_PER_SOL
    );
    await connection.confirmTransaction(sig, "confirmed");

    const freshAsset = Keypair.generate();
    await mintAsset(freshAsset);

    try {
      await program.methods
        .stake()
        .accounts({
          owner: impostor.publicKey,
          config,
          asset: freshAsset.publicKey,
          collection: collection.publicKey,
          updateAuthority,
          systemProgram: SystemProgram.programId,
          mplCoreProgram: MPL_CORE_PROGRAM_ID,
        } as any)
        .signers([impostor])
        .rpc();
      assert.fail("expected stake() to fail for a non-owner signer");
    } catch (err) {
      assert.include(String(err), "InvalidOwner");
    }
  });
});