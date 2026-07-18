use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    boson_runtime::Builder::from_env().run_worker().await
}
