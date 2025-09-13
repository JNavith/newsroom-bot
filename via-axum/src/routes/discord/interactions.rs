use crate::AppState;
use axum::{Router, response::IntoResponse, routing::get};
use ed25519_compact::{PublicKey, Signature};
use snafu::Snafu;

pub fn create_router() -> Router<AppState> {
    Router::new().route("/", get(handle_get))
}

#[derive(Debug, Snafu)]
pub enum VerificationError {
    #[snafu(display("signature was not provided"))]
    MissingSignature,
    #[snafu(display("timestamp was not provided"))]
    MissingTimestamp,
    #[snafu(display("body was not provided"))]
    MissingBody,
    #[snafu(display("public key was not provided"))]
    MissingPublicKey,

    #[snafu(display("the given signature is not represented in hex"))]
    SignatureInvalidHex,
    #[snafu(display("the given signature does not represent a valid ED25519 compact signature"))]
    SignatureInvalidKey,

    #[snafu(display("the given public key is not represented in hex"))]
    PublicKeyInvalidHex,
    #[snafu(display("the given public key does not represent a valid ED25519 compact public key"))]
    PublicKeyInvalidKey,

    #[snafu(display(
        "all the needed information was provided, but this message was not signed with the private key corresponding to this public key, so something suspicious may be going on"
    ))]
    DoesNotVerify,
}

pub fn verify(
    signature: Option<&str>,
    timestamp: Option<&str>,
    body: Option<&str>,
    public_key: Option<&str>,
) -> Result<(), VerificationError> {
    use VerificationError::*;

    let signature = signature.ok_or(MissingSignature)?;
    let timestamp = timestamp.ok_or(MissingTimestamp)?;
    let body = body.ok_or(MissingBody)?;
    let public_key = public_key.ok_or(MissingPublicKey)?;

    let message = [timestamp.as_bytes(), body.as_bytes()].concat();

    let signature = hex::decode(signature).map_err(|_e| SignatureInvalidHex)?;
    let signature = Signature::from_slice(&signature).map_err(|_e| SignatureInvalidKey)?;

    let public_key = hex::decode(public_key).map_err(|_e| PublicKeyInvalidHex)?;
    let public_key = PublicKey::from_slice(&public_key).map_err(|_e| PublicKeyInvalidKey)?;

    public_key
        .verify(message, &signature)
        .map_err(|_e| DoesNotVerify)
}

#[tracing::instrument]
pub async fn handle_get() -> impl IntoResponse {
    todo!();
    ()
}
