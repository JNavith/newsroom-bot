use std::net::{IpAddr, SocketAddr};

use clap::Parser;
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

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let Args {
        ip,
        port,
        discord_token,
    } = Args::parse();

    tracing_subscriber::fmt::init();

    let addr = SocketAddr::new(ip, port);
    let listener = TcpListener::bind(addr).await.context(BindSnafu)?;

    let router = via_axum::init(discord_token).await.context(AxumInitSnafu)?;

    tracing::info!(?addr, "listening on");
    axum::serve(listener, router).await.context(ServeSnafu)?;

    Ok(())
}
