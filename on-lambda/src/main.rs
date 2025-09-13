use lambda_http::{Error, run, tracing};
use via_axum::create_router;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing::init_default_subscriber();

    let router = create_router();

    run(router).await
}
