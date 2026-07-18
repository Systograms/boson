use std::sync::Arc;

use anyhow::Result;
use todo_todos::TodosCapability;

#[tokio::main]
async fn main() -> Result<()> {
    boson_runtime::Builder::from_env()
        .extend(|ctx, registry| {
            registry.register(Arc::new(TodosCapability::new(
                ctx.database.clone(),
                ctx.identity_auth.clone(),
                &ctx.config,
            )?))?;
            Ok(())
        })
        .run_server()
        .await
}
