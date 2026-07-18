use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    boson_runtime::Builder::from_env()
        .register(my_api_app::register_app)
        .run_worker()
        .await
}
