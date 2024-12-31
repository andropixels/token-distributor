use anchor_lang::prelude::*;
use anchor_spl::{
    token::{self, Token, TokenAccount, Mint, Transfer},
    associated_token::AssociatedToken,
};
use anchor_lang::solana_program::keccak; 

declare_id!("HcM41aMorQuxwoGqYhV3Qtt63M8Q1Cu5JpEPca7MZ3MY");

#[program]
pub mod token_distributor {
    #[event]
    pub struct TokensClaimed {
        pub claimer: Pubkey,
        pub amount: u64,
    }

    #[event]
    pub struct MerkleRootUpdated {
        pub new_root: [u8; 32],
    }

    #[event]
    pub struct TokensDeposited {
        pub amount: u64,
    }
    use super::*;

    // pub fn initialize(
    //     ctx: Context<Initialize>,
    //     merkle_root: [u8; 32],
    // ) -> Result<()> {
    //     let distributor = &mut ctx.accounts.distributor;
    //     distributor.authority = ctx.accounts.authority.key();
    //     distributor.merkle_root = merkle_root;
    //     distributor.bump = ctx.bumps.distributor;
    //     Ok(())
    // }

    pub fn initialize(
        ctx: Context<Initialize>,
        merkle_root: [u8; 32],
        unique_seed: [u8; 8],
    ) -> Result<()> {
        let distributor = &mut ctx.accounts.distributor;
        distributor.authority = ctx.accounts.authority.key();
        distributor.merkle_root = merkle_root;
        distributor.bump = ctx.bumps.distributor;
        distributor.unique_seed = unique_seed;
        Ok(())
    }

    pub fn update_merkle_root(
        ctx: Context<UpdateMerkleRoot>, 
        new_root: [u8; 32]
    ) -> Result<()> {
        require!(
            ctx.accounts.authority.key() == ctx.accounts.distributor.authority,
            DistributorError::Unauthorized
        );
        ctx.accounts.distributor.merkle_root = new_root;
        emit!(MerkleRootUpdated { new_root });
        Ok(())
    }

    pub fn recover_tokens(
        ctx: Context<DepositTokens>,
        amount: u64,
    ) -> Result<()> {
        require!(
            ctx.accounts.authority.key() == ctx.accounts.distributor.authority,
            DistributorError::Unauthorized
        );

        let authority_seeds = &[
            b"distributor".as_ref(),
            &[ctx.accounts.distributor.bump],
        ];
        let signer = &[&authority_seeds[..]];

        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.from.to_account_info(),
                authority: ctx.accounts.distributor.to_account_info(),
            },
            signer,
        );

        token::transfer(transfer_ctx, amount)?;
        emit!(TokensDeposited { amount });
        Ok(())
    }

    pub fn deposit_tokens(
        ctx: Context<DepositTokens>,
        amount: u64,
    ) -> Result<()> {
        require!(
            ctx.accounts.authority.key() == ctx.accounts.distributor.authority,
            DistributorError::Unauthorized
        );

        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.from.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        );

        token::transfer(transfer_ctx, amount)?;
        Ok(())
    }
    
    pub fn claim(
        ctx: Context<Claim>,
        amount: u64,
        proof: Vec<[u8; 32]>,
    ) -> Result<()> {
        // Derive PDA seeds for signing
        let distributor_seeds = &[
            b"distributor".as_ref(),
            ctx.accounts.distributor.unique_seed.as_ref(),  // Include unique seed
            &[ctx.accounts.distributor.bump],
        ];
        let distributor_signer = &[&distributor_seeds[..]];
    
        // Verify merkle proof
        let leaf = keccak::hashv(&[
            &ctx.accounts.claimer.key().to_bytes(),
            &amount.to_le_bytes(),
        ]).0;
    
        require!(
            verify_proof(&proof, ctx.accounts.distributor.merkle_root, leaf),
            DistributorError::InvalidProof
        );
    
        // Transfer tokens with proper signer
        let transfer_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault.to_account_info(),
                to: ctx.accounts.claimer_token_account.to_account_info(),
                authority: ctx.accounts.distributor.to_account_info(),
            },
            distributor_signer,
        );
    
        token::transfer(transfer_ctx, amount)?;
    
        // Mark as claimed
        ctx.accounts.claim_status.is_claimed = true;
        
        emit!(TokensClaimed { 
            claimer: ctx.accounts.claimer.key(),
            amount 
        });
    
        Ok(())
    }

}
#[derive(Accounts)]
#[instruction(merkle_root: [u8; 32], unique_seed: [u8; 8])]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + DistributorAccount::SIZE,
        seeds = [b"distributor", unique_seed.as_ref()],
        bump
    )]
    pub distributor: Account<'info, DistributorAccount>,

    #[account(
        init,
        payer = authority,
        associated_token::mint = mint,
        associated_token::authority = distributor
    )]
    pub vault: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}
#[derive(Accounts)]
pub struct UpdateMerkleRoot<'info> {
    #[account(mut)]
    pub distributor: Account<'info, DistributorAccount>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct DepositTokens<'info> {
    #[account(mut)]
    pub distributor: Account<'info, DistributorAccount>,
    
    #[account(mut)]
    pub from: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = distributor
    )]
    pub vault: Account<'info, TokenAccount>,
    
    pub mint: Account<'info, Mint>,
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub distributor: Account<'info, DistributorAccount>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = distributor
    )]
    pub vault: Account<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = claimer,
        space = 8 + ClaimStatus::SIZE,
        seeds = [
            b"claim_status",
            distributor.key().as_ref(),
            claimer.key().as_ref(),
        ],
        bump
    )]
    pub claim_status: Account<'info, ClaimStatus>,
    
    #[account(
        init_if_needed,
        payer = claimer,
        associated_token::mint = mint,
        associated_token::authority = claimer
    )]
    pub claimer_token_account: Account<'info, TokenAccount>,
    
    pub mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub claimer: Signer<'info>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[account]
pub struct DistributorAccount {
    pub authority: Pubkey,
    pub merkle_root: [u8; 32],
    pub bump: u8,
    pub unique_seed: [u8; 8]  // Add this field
}

impl DistributorAccount {
    pub const SIZE: usize = 32 + 32 + 1 + 8; 
}

#[account]
pub struct ClaimStatus {
    pub is_claimed: bool,
}

impl ClaimStatus {
    pub const SIZE: usize = 1;
}

#[error_code]
pub enum DistributorError {
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Invalid merkle proof")]
    InvalidProof,
    #[msg("Tokens have already been claimed")]
    AlreadyClaimed,
    #[msg("Insufficient balance in vault")]
    InsufficientVaultBalance,
}

fn verify_proof(
    proof: &[[u8; 32]],
    root: [u8; 32],
    leaf: [u8; 32],
) -> bool {
    let mut computed_hash = leaf;
    for proof_element in proof.iter() {
        if computed_hash <= *proof_element {
            computed_hash = keccak::hashv(&[&computed_hash, proof_element]).0;
        } else {
            computed_hash = keccak::hashv(&[proof_element, &computed_hash]).0;
        }
    }
    computed_hash == root
}