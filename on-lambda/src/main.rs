use clap::Parser;
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
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    let Args { discord_token } = Args::parse();

    lambda_http::tracing::init_default_subscriber();

    let router = via_axum::init(discord_token).await.context(AxumInitSnafu)?;

    lambda_http::run(router).await?;

    Ok(())
}
