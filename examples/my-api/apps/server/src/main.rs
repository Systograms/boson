use std::sync::Arc;

use anyhow::Result;
use my_api_items::ItemsCapability;

#[tokio::main]
async fn main() -> Result<()> {
    boson_runtime::Builder::from_env()
        .extend(|ctx, registry| {
            registry.register(Arc::new(ItemsCapability::new(
                ctx.database.clone(),
                ctx.identity_auth.clone(),
                &ctx.config,
            )?))?;
            Ok(())
        })
        .run_server()
        .await
}
