import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { TokenDistributor } from "../target/types/token_distributor";
import { 
    PublicKey, 
    SystemProgram, 
    SYSVAR_RENT_PUBKEY, 
    Keypair 
} from "@solana/web3.js";
import { 
    TOKEN_PROGRAM_ID, 
    ASSOCIATED_TOKEN_PROGRAM_ID, 
    createMint,
    getAssociatedTokenAddress,
    createAssociatedTokenAccount,
    mintTo,
    createAssociatedTokenAccountInstruction
} from "@solana/spl-token";
import { MerkleTree } from 'merkletreejs';
import { keccak_256 } from "js-sha3";

describe("token_distributor", () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.TokenDistributor as Program<TokenDistributor>;

    // Setup accounts
    const mintKeypair = Keypair.generate();
    const testUser = Keypair.generate();
    const uniqueSeed = new Array(8).fill(0);
    let mint: PublicKey;
    let distributor: PublicKey;
    let vault: PublicKey;
    let authorityATA: PublicKey;
    let testUserATA: PublicKey;
    let claimStatus: PublicKey;

    // Create whitelist with multiple users
    const whitelist = [
        { address: testUser.publicKey, amount: new anchor.BN(100_000_000_000) },
        { address: Keypair.generate().publicKey, amount: new anchor.BN(200_000_000_000) },
        { address: Keypair.generate().publicKey, amount: new anchor.BN(300_000_000_000) }
    ];

    // Setup merkle tree
    let merkleTree: MerkleTree;
    let merkleRoot: number[];
    let merkleProof: Buffer[];

    function setupMerkleTree() {
        // Create leaves
        const leaves = whitelist.map(entry => {
            const leaf = Buffer.concat([
                entry.address.toBuffer(),
                Buffer.from(entry.amount.toArray('le', 8))
            ]);
            return Buffer.from(keccak_256(leaf), 'hex');
        });

        // Create tree
        merkleTree = new MerkleTree(leaves, keccak_256, { sortPairs: true });
        merkleRoot = Array.from(merkleTree.getRoot());

        // Get proof for test user
        const testUserLeaf = Buffer.concat([
            testUser.publicKey.toBuffer(),
            Buffer.from(whitelist[0].amount.toArray('le', 8))
        ]);
        const hashedLeaf = Buffer.from(keccak_256(testUserLeaf), 'hex');
        merkleProof = merkleTree.getProof(hashedLeaf).map(p => p.data);
    }

    it("Create mint and ATAs", async () => {
        try {
            mint = await createMint(
                provider.connection,
                provider.wallet.payer,
                provider.wallet.publicKey,
                provider.wallet.publicKey,
                9
            );

            authorityATA = await getAssociatedTokenAddress(
                mint,
                provider.wallet.publicKey
            );

            await createAssociatedTokenAccount(
                provider.connection,
                provider.wallet.payer,
                mint,
                provider.wallet.publicKey
            );

            await mintTo(
                provider.connection,
                provider.wallet.payer,
                mint,
                authorityATA,
                provider.wallet.publicKey,
                1000_000_000_000
            );

            setupMerkleTree();
        } catch (error) {
            console.error("Failed to create mint:", error);
            throw error;
        }
    });

    it("Initialize distributor", async () => {
        try {
            [distributor] = await PublicKey.findProgramAddress(
                [Buffer.from("distributor"), Buffer.from(uniqueSeed)],
                program.programId
            );

            vault = await getAssociatedTokenAddress(
                mint,
                distributor,
                true
            );

            const tx = await program.methods
                .initialize(merkleRoot, uniqueSeed)
                .accounts({
                    distributor,
                    vault,
                    mint,
                    authority: provider.wallet.publicKey,
                    systemProgram: SystemProgram.programId,
                    tokenProgram: TOKEN_PROGRAM_ID,
                    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
                    rent: SYSVAR_RENT_PUBKEY,
                })
                .rpc();

            console.log("Initialization successful:", tx);
        } catch (error) {
            console.error("Failed to initialize:", error);
            throw error;
        }
    });

    it("Deposit tokens", async () => {
        try {
            const depositAmount = new anchor.BN(500_000_000_000);

            const tx = await program.methods
                .depositTokens(depositAmount)
                .accounts({
                    distributor,
                    from: authorityATA,
                    vault,
                    mint,
                    authority: provider.wallet.publicKey,
                    tokenProgram: TOKEN_PROGRAM_ID,
                })
                .rpc();

            console.log("Deposit successful:", tx);
        } catch (error) {
            console.error("Failed to deposit:", error);
            throw error;
        }
    });

    it("Claim tokens", async () => {
        try {
            const signature = await provider.connection.requestAirdrop(testUser.publicKey, 2000000000);
            await provider.connection.confirmTransaction(signature);

            testUserATA = await getAssociatedTokenAddress(
                mint,
                testUser.publicKey
            );

            const createAtaIx = createAssociatedTokenAccountInstruction(
                provider.wallet.publicKey,
                testUserATA,
                testUser.publicKey,
                mint
            );

            const createAtaTx = new anchor.web3.Transaction().add(createAtaIx);
            await provider.sendAndConfirm(createAtaTx);
            [claimStatus] = await PublicKey.findProgramAddress(
                [
                    Buffer.from("claim_status"), // Match the program seed
                    distributor.toBuffer(),
                    testUser.publicKey.toBuffer(),
                ],
                program.programId
            );
            
            console.log("PDA Derivation Details:");
            console.log("Program ID:", program.programId.toString());
            console.log("Distributor:", distributor.toString());
            console.log("User:", testUser.publicKey.toString());
            console.log("Derived Claim Status:", claimStatus.toString());
            
            const tx = await program.methods
                .claim(whitelist[0].amount, merkleProof.map(p => Array.from(p)))
                .accounts({
                    distributor,
                    vault,
                    claimStatus,
                    claimerTokenAccount: testUserATA,
                    mint,
                    claimer: testUser.publicKey,
                    systemProgram: SystemProgram.programId,
                    tokenProgram: TOKEN_PROGRAM_ID,
                    associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
                    rent: SYSVAR_RENT_PUBKEY,
                })
                .signers([testUser])
                .rpc();

            const balance = await provider.connection.getTokenAccountBalance(testUserATA);
            console.log("Claim successful! User balance:", balance.value.amount);
        } catch (error) {
            console.error("Failed to claim:", error);
            throw error;
        }
    });
});