// Deterministic bitcoin commitments library, implementing LNPBP standards
// Part of bitcoin protocol core library (BP Core Lib)
//
// Written in 2020-2022 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the Apache 2.0 License
// along with this software.
// If not, see <https://opensource.org/licenses/Apache-2.0>.

use amplify::Wrapper;
use bitcoin::hashes::Hash;
use bitcoin::psbt::TapTree;
use bitcoin::util::taproot::TapBranchHash;
use bitcoin::Script;
use bitcoin_scripts::taproot::{Node, TaprootScriptTree, TreeNode};
use bitcoin_scripts::{TapNodeHash, TapScript};
use commit_verify::{
    lnpbp4, CommitVerify, EmbedCommitProof, EmbedCommitVerify,
};
use psbt::commit::tapret::DfsPathEncodeError;
use secp256k1::SECP256K1;

use super::{Lnpbp6, TapretProof};
use crate::tapret::taptree::{
    TapretProofError, TapretSourceError, TapretSourceInfo,
};

/// Errors during tapret PSBT commitment process.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Display, Error, From)]
pub enum PsbtCommitError {
    /// Invalid taproot script tree source information.
    #[from]
    #[display(inner)]
    SourceError(TapretSourceError),

    /// tapret commitment can't be made in a transaction lacking any taproot
    /// outputs.
    #[display(doc_comments)]
    NoTaprootOutput,

    /// tapret commitment can't be made due to an absent taproot internal key
    /// in PSBT data.
    InternalKeyMissed,

    /// tapret commitment does not change internal key, but the key in PSBT
    /// data and key from the tapret proof differ.
    InternalKeyMismatch,

    /// invalid tapret commitment path in PSBT data.
    #[from(DfsPathEncodeError)]
    TapretPathInvalid,

    /// PSBT output misses tapret path information.
    TapretPathMissed,
}

/// Errors during tapret PSBT commitment process.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Display, Error, From)]
#[display(inner)]
pub enum PsbtVerifyError {
    /// Error during commitment process.
    #[from]
    #[from(DfsPathEncodeError)]
    #[from(TapretSourceError)]
    Commit(PsbtCommitError),

    /// Error during verification process.
    #[from]
    Proof(TapretProofError),
}

impl EmbedCommitProof<lnpbp4::CommitmentHash, psbt::Output, Lnpbp6>
    for TapretProof
{
    fn restore_original_container(
        &self,
        commit_container: &psbt::Output,
    ) -> Result<psbt::Output, PsbtVerifyError> {
        let mut original_container = commit_container.clone();

        let internal_key = original_container
            .tap_internal_key
            .ok_or(PsbtCommitError::InternalKeyMissed)?;
        if internal_key != self.internal_key {
            return Err(PsbtCommitError::InternalKeyMismatch)
                .map_err(PsbtVerifyError::from);
        }

        let tap_tree = original_container.tap_tree.map(TaprootScriptTree::from);
        let source = TapretSourceInfo::<TaprootScriptTree>::with(tap_tree)?;
        let source = self.path_proof.restore_original_container(&source)?;

        let merkle_root = source
            .as_root_node()
            .map(TreeNode::node_hash)
            .map(TapNodeHash::into_inner)
            .map(TapBranchHash::from_inner);
        original_container.script =
            Script::new_v1_p2tr(SECP256K1, self.internal_key, merkle_root);

        original_container.tap_tree = source.into_tap_tree();

        Ok(original_container)
    }
}

impl EmbedCommitVerify<lnpbp4::CommitmentHash, Lnpbp6> for psbt::Output {
    type Proof = TapretProof;
    type CommitError = PsbtCommitError;
    type VerifyError = PsbtVerifyError;

    fn embed_commit(
        &mut self,
        msg: &lnpbp4::CommitmentHash,
    ) -> Result<Self::Proof, Self::CommitError> {
        // TODO: Check TAPRET_COMMITABLE key

        let internal_key = if let Some(internal_key) = self.tap_internal_key {
            internal_key
        } else {
            return Err(PsbtCommitError::InternalKeyMissed);
        };

        let mut source =
            TapretSourceInfo::<TapTree>::with(self.tap_tree.clone())?;

        let path_proof = source.embed_commit(msg)?;

        self.script = TapScript::commit(&(*msg, path_proof.nonce)).into_inner();

        let proof = TapretProof {
            path_proof,
            internal_key,
        };

        Ok(proof)
    }
}
