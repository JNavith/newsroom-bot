use std::net::{IpAddr, SocketAddr};

use clap::Parser;
use parse_hex_public_key::{Hex, PublicKeyOrphanRuleAvoidance};
use secrecy::SecretString;
use snafu::{ResultExt, Snafu};
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, env, default_value_t = [127, 0, 0, 1].into())]
    ip: IpAddr,
    #[arg(long, env)]
    port: u16,

    #[arg(long, env)]
    discord_token: SecretString,

    #[arg(long, env)]
    discord_application_public_key: Hex<PublicKeyOrphanRuleAvoidance>,
}

#[derive(Debug, Snafu)]
enum AppError {
    #[snafu(display("couldn't initialize the web server"))]
    AxumInitError { source: via_axum::InitError },

    #[snafu(display("couldn't bind to the specified ip and port"))]
    BindError { source: std::io::Error },

    #[snafu(display("couldn't run the web server"))]
    ServeError { source: std::io::Error },
}

#[snafu::report]
#[tokio::main]
async fn main() -> Result<(), AppError> {
    let Args {
        ip,
        port,
        discord_token,
        discord_application_public_key:
            Hex(PublicKeyOrphanRuleAvoidance(discord_application_public_key)),
    } = Args::parse();

    tracing_subscriber::fmt().pretty().init();

    let addr = SocketAddr::new(ip, port);
    let listener = TcpListener::bind(addr).await.context(BindSnafu)?;

    let router = via_axum::init(discord_token, discord_application_public_key)
        .await
        .context(AxumInitSnafu)?;

    tracing::info!(?addr, "listening on");
    axum::serve(listener, router).await.context(ServeSnafu)?;

    Ok(())
}
