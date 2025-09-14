use crate::AppState;
use axum::{
    Json, RequestExt, Router,
    body::Bytes,
    extract::{FromRef, FromRequest, Request, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use axum_extra::TypedHeader;
use discord_bot::Interaction;
use ed25519_compact::{PublicKey, Signature};
use headers::Header;
use serde::de::DeserializeOwned;
use snafu::{Report, ResultExt, Snafu};

pub fn create_router() -> Router<AppState> {
    Router::new().route("/", post(handle_post))
}

impl FromRef<AppState> for PublicKey {
    fn from_ref(input: &AppState) -> Self {
        input.discord_application_public_key.to_owned()
    }
}

#[derive(Debug, Snafu)]
#[snafu(display(
    "all the needed information was provided, but this message was not signed with the private key corresponding to this public key, so something suspicious may be going on"
))]
struct VerificationError {
    source: ed25519_compact::Error,
}

impl IntoResponse for VerificationError {
    fn into_response(self) -> Response {
        let status_code = StatusCode::FORBIDDEN;

        let report = Report::from_error(self);
        let body = report.to_string();

        (status_code, body).into_response()
    }
}

fn verify(
    body: &[u8],
    timestamp: &[u8],
    signature: Signature,
    public_key: &PublicKey,
) -> Result<(), VerificationError> {
    let message = [timestamp, body].concat();

    public_key
        .verify(message, &signature)
        .context(VerificationSnafu)
}

#[derive(Debug)]
struct XSignatureEd25519(Vec<u8>);
static X_SIGNATURE_ED25519_HEADER_NAME: HeaderName = HeaderName::from_static("x-signature-ed25519");
impl Header for XSignatureEd25519 {
    fn name() -> &'static HeaderName {
        &X_SIGNATURE_ED25519_HEADER_NAME
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;

        Ok(Self(value.as_bytes().to_owned()))
    }

    fn encode<E>(&self, values: &mut E)
    where
        E: Extend<HeaderValue>,
    {
        let value = HeaderValue::from_bytes(&self.0).unwrap();

        values.extend(std::iter::once(value));
    }
}

#[derive(Debug)]
struct XSignatureTimestamp(Vec<u8>);
static X_SIGNATURE_TIMESTAMP_HEADER_NAME: HeaderName =
    HeaderName::from_static("x-signature-timestamp");
impl Header for XSignatureTimestamp {
    fn name() -> &'static HeaderName {
        &X_SIGNATURE_TIMESTAMP_HEADER_NAME
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;

        Ok(Self(value.as_bytes().to_owned()))
    }

    fn encode<E>(&self, values: &mut E)
    where
        E: Extend<HeaderValue>,
    {
        let value = HeaderValue::from_bytes(&self.0).unwrap();

        values.extend(std::iter::once(value));
    }
}

#[derive(Debug, Clone, Snafu)]
#[snafu(display("the given signature is not represented in hex"))]
struct SignatureInvalidHex {
    source: hex::FromHexError,
}
impl IntoResponse for SignatureInvalidHex {
    fn into_response(self) -> Response {
        let status_code = StatusCode::BAD_REQUEST;

        let report = Report::from_error(self);
        let body = report.to_string();

        (status_code, body).into_response()
    }
}

#[derive(Debug, Clone, Snafu)]
#[snafu(display("the given signature does not represent a valid ED25519 compact signature"))]
struct SignatureInvalidKey {
    source: ed25519_compact::Error,
}
impl IntoResponse for SignatureInvalidKey {
    fn into_response(self) -> Response {
        let status_code = StatusCode::BAD_REQUEST;

        let report = Report::from_error(self);
        let body = report.to_string();

        (status_code, body).into_response()
    }
}

#[derive(Debug)]
struct Ed25519Verified(Bytes);

impl FromRequest<PublicKey> for Ed25519Verified {
    type Rejection = Response;

    fn from_request(
        mut req: Request,
        public_key: &PublicKey,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let TypedHeader(XSignatureEd25519(signature)) = req
                .extract_parts()
                .await
                .map_err(IntoResponse::into_response)?;

            let signature = hex::decode(signature)
                .context(SignatureInvalidHexSnafu)
                .map_err(IntoResponse::into_response)?;
            let signature = Signature::from_slice(&signature)
                .context(SignatureInvalidKeySnafu)
                .map_err(IntoResponse::into_response)?;

            let TypedHeader(XSignatureTimestamp(timestamp)) = req
                .extract_parts()
                .await
                .map_err(IntoResponse::into_response)?;

            let body = Bytes::from_request(req, public_key)
                .await
                .map_err(IntoResponse::into_response)?;

            verify(&body, &timestamp, signature, public_key)
                .map_err(IntoResponse::into_response)?;

            Ok(Self(body))
        }
    }
}

pub struct Ed25519VerifiedJson<D: DeserializeOwned>(pub D);

impl<D, S> FromRequest<S> for Ed25519VerifiedJson<D>
where
    D: DeserializeOwned + Send,
    PublicKey: FromRef<S>,
    S: Sync + Send,
{
    type Rejection = Response;

    fn from_request(
        req: Request,
        state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            let public_key = PublicKey::from_ref(state);
            let Ed25519Verified(body) = req.extract_with_state(&public_key).await?;

            let deserialized = serde_json::from_slice(&body).map_err(|deserialization_error| {
                (StatusCode::BAD_REQUEST, deserialization_error.to_string()).into_response()
            })?;

            Ok(Self(deserialized))
        }
    }
}

#[tracing::instrument(skip(app_state))]
pub async fn handle_post(
    State(app_state): State<AppState>,
    Ed25519VerifiedJson(interaction): Ed25519VerifiedJson<Interaction>,
) -> impl IntoResponse {
    let discord_token = app_state.discord_token;
    let discord_state = discord_bot::State { discord_token };

    match app_state
        .discord_interaction_handler
        .handle(discord_state, interaction)
        .await
    {
        Ok(response) => Json(response),
        Err(error) => todo!(),
    }
}
