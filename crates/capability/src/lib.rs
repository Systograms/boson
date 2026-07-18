//! The stable registration contract for first- and third-party capabilities.

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use axum::Router;
use boson_events::{EventConsumer, EventError};
use boson_ports::{HealthCheck, JobEnvelope};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CapabilityDescriptor {
    pub name: &'static str,
    pub version: &'static str,
    /// Capabilities that must be registered and migrated first.
    pub dependencies: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct MigrationSet {
    pub owner: &'static str,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Schedule {
    pub name: &'static str,
    pub cron: &'static str,
    pub job: &'static str,
}

#[async_trait]
pub trait JobHandler: Send + Sync {
    fn name(&self) -> &'static str;
    async fn handle(&self, job: &JobEnvelope) -> Result<(), EventError>;
}

/// Capabilities are linked and registered at the composition root.
///
/// Returned routers must be fully bound to their capability state, so the host
/// can merge them without exposing its own application state type.
pub trait Capability: Send + Sync {
    fn descriptor(&self) -> CapabilityDescriptor;

    fn app_router(&self) -> Router {
        Router::new()
    }

    fn admin_router(&self) -> Router {
        Router::new()
    }

    fn migrations(&self) -> Option<MigrationSet> {
        None
    }

    fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
        Vec::new()
    }

    fn job_handlers(&self) -> Vec<Arc<dyn JobHandler>> {
        Vec::new()
    }

    fn schedules(&self) -> Vec<Schedule> {
        Vec::new()
    }

    fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        Vec::new()
    }
}

#[derive(Debug, Error)]
pub enum RegistrationError {
    #[error("capability `{0}` is already registered")]
    Duplicate(&'static str),
    #[error("capability `{capability}` requires `{dependency}` to be registered first")]
    MissingDependency {
        capability: &'static str,
        dependency: &'static str,
    },
}

#[derive(Default)]
pub struct CapabilityRegistry {
    capabilities: Vec<Arc<dyn Capability>>,
}

impl CapabilityRegistry {
    /// Registers a capability after its declared dependencies.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] if the name is already registered or a
    /// declared dependency has not been registered first.
    pub fn register(&mut self, capability: Arc<dyn Capability>) -> Result<(), RegistrationError> {
        let descriptor = capability.descriptor();
        if self
            .capabilities
            .iter()
            .any(|registered| registered.descriptor().name == descriptor.name)
        {
            return Err(RegistrationError::Duplicate(descriptor.name));
        }
        for dependency in descriptor.dependencies {
            if !self
                .capabilities
                .iter()
                .any(|registered| registered.descriptor().name == *dependency)
            {
                return Err(RegistrationError::MissingDependency {
                    capability: descriptor.name,
                    dependency,
                });
            }
        }
        self.capabilities.push(capability);
        Ok(())
    }

    pub fn app_router(&self) -> Router {
        self.capabilities
            .iter()
            .fold(Router::new(), |router, capability| {
                router.merge(capability.app_router())
            })
    }

    pub fn admin_router(&self) -> Router {
        self.capabilities
            .iter()
            .fold(Router::new(), |router, capability| {
                router.merge(capability.admin_router())
            })
    }

    #[must_use]
    pub fn descriptors(&self) -> Vec<CapabilityDescriptor> {
        self.capabilities
            .iter()
            .map(|capability| capability.descriptor())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCapability {
        name: &'static str,
        dependencies: &'static [&'static str],
    }

    impl Capability for TestCapability {
        fn descriptor(&self) -> CapabilityDescriptor {
            CapabilityDescriptor {
                name: self.name,
                version: "1.0.0",
                dependencies: self.dependencies,
            }
        }
    }

    #[test]
    fn enforces_registration_order() {
        let mut registry = CapabilityRegistry::default();
        let child = Arc::new(TestCapability {
            name: "child",
            dependencies: &["parent"],
        });
        assert!(matches!(
            registry.register(child),
            Err(RegistrationError::MissingDependency { .. })
        ));
    }
}
