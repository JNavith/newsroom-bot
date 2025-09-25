use clap::Parser;
use parse_hex_public_key::{Hex, PublicKeyOrphanRuleAvoidance};
use secrecy::SecretString;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
enum AppError {
    #[snafu(display("couldn't initialize the axum web server"))]
    AxumInitError { source: via_axum::InitError },
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(env)]
    discord_token: SecretString,
    #[arg(env)]
    discord_application_public_key: Hex<PublicKeyOrphanRuleAvoidance>,

    #[arg(env)]
    spotify_client_id: String,
    #[arg(env)]
    spotify_client_secret: SecretString,
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    let Args {
        discord_token,
        discord_application_public_key:
            Hex(PublicKeyOrphanRuleAvoidance(discord_application_public_key)),
        spotify_client_id,
        spotify_client_secret,
    } = Args::parse();

    lambda_http::tracing::init_default_subscriber();

    let router = via_axum::init(via_axum::InitArgs {
        discord_token,
        discord_application_public_key,
        spotify_client_id,
        spotify_client_secret,
    })
    .await
    .context(AxumInitSnafu)?;

    lambda_http::run(router).await?;

    Ok(())
}
