//! Shared application composition for migrate, Server, and Worker hosts.

use std::sync::Arc;

use anyhow::Result;
use boson_runtime::{CapabilityRegistry, RuntimeContext};
use my_api_items::ItemsCapability;

/// Registers every application capability for this project.
///
/// # Errors
///
/// Returns an error when a capability cannot be constructed or registered.
pub fn register_app(ctx: &RuntimeContext, registry: &mut CapabilityRegistry) -> Result<()> {
    registry.register(Arc::new(ItemsCapability::new(
        ctx.database.clone(),
        ctx.identity_auth.clone(),
        &ctx.config,
    )?))?;
    Ok(())
}
