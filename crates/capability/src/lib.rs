//! The stable registration contract for first- and third-party capabilities.

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

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

    /// Admin API scopes this capability expects issued keys to optionally hold.
    fn scopes(&self) -> &'static [&'static str] {
        &[]
    }

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
    #[error("duplicate job handler `{0}`")]
    DuplicateJobHandler(&'static str),
    #[error("duplicate event consumer `{0}`")]
    DuplicateEventConsumer(&'static str),
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
    /// Returns [`RegistrationError`] if the name is already registered, a
    /// declared dependency has not been registered first, or a job/event
    /// handler name collides with an earlier registration.
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

        let mut existing_jobs = self
            .job_handlers()
            .into_iter()
            .map(|handler| handler.name())
            .collect::<BTreeSet<_>>();
        for handler in capability.job_handlers() {
            if !existing_jobs.insert(handler.name()) {
                return Err(RegistrationError::DuplicateJobHandler(handler.name()));
            }
        }

        let mut existing_consumers = self
            .event_consumers()
            .into_iter()
            .map(|consumer| consumer.name())
            .collect::<BTreeSet<_>>();
        for consumer in capability.event_consumers() {
            if !existing_consumers.insert(consumer.name()) {
                return Err(RegistrationError::DuplicateEventConsumer(consumer.name()));
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

    /// Collects unique Admin scopes declared by registered capabilities.
    #[must_use]
    pub fn scopes(&self) -> Vec<&'static str> {
        let mut scopes = BTreeSet::new();
        for capability in &self.capabilities {
            for scope in capability.scopes() {
                scopes.insert(*scope);
            }
        }
        scopes.into_iter().collect()
    }

    #[must_use]
    pub fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
        self.capabilities
            .iter()
            .flat_map(|capability| capability.event_consumers())
            .collect()
    }

    #[must_use]
    pub fn job_handlers(&self) -> Vec<Arc<dyn JobHandler>> {
        self.capabilities
            .iter()
            .flat_map(|capability| capability.job_handlers())
            .collect()
    }

    #[must_use]
    pub fn schedules(&self) -> Vec<Schedule> {
        self.capabilities
            .iter()
            .flat_map(|capability| capability.schedules())
            .collect()
    }

    #[must_use]
    pub fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        self.capabilities
            .iter()
            .flat_map(|capability| capability.health_checks())
            .collect()
    }

    #[must_use]
    pub fn migrations(&self) -> Vec<MigrationSet> {
        self.capabilities
            .iter()
            .filter_map(|capability| capability.migrations())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use boson_events::{EventEnvelope, EventError};

    struct TestCapability {
        name: &'static str,
        dependencies: &'static [&'static str],
        scopes: &'static [&'static str],
        job: Option<&'static str>,
        consumer: Option<&'static str>,
    }

    impl Capability for TestCapability {
        fn descriptor(&self) -> CapabilityDescriptor {
            CapabilityDescriptor {
                name: self.name,
                version: "1.0.0",
                dependencies: self.dependencies,
            }
        }

        fn scopes(&self) -> &'static [&'static str] {
            self.scopes
        }

        fn job_handlers(&self) -> Vec<Arc<dyn JobHandler>> {
            self.job
                .map(|name| Arc::new(NamedJob(name)) as Arc<dyn JobHandler>)
                .into_iter()
                .collect()
        }

        fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
            self.consumer
                .map(|name| Arc::new(NamedConsumer(name)) as Arc<dyn EventConsumer>)
                .into_iter()
                .collect()
        }
    }

    struct NamedJob(&'static str);

    #[async_trait]
    impl JobHandler for NamedJob {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn handle(&self, _job: &JobEnvelope) -> Result<(), EventError> {
            Ok(())
        }
    }

    struct NamedConsumer(&'static str);

    #[async_trait]
    impl EventConsumer for NamedConsumer {
        fn name(&self) -> &'static str {
            self.0
        }

        fn topic(&self) -> &'static str {
            "test.topic.v1"
        }

        async fn handle(&self, _event: &EventEnvelope) -> Result<(), EventError> {
            Ok(())
        }
    }

    #[test]
    fn enforces_registration_order() {
        let mut registry = CapabilityRegistry::default();
        let child = Arc::new(TestCapability {
            name: "child",
            dependencies: &["parent"],
            scopes: &[],
            job: None,
            consumer: None,
        });
        assert!(matches!(
            registry.register(child),
            Err(RegistrationError::MissingDependency { .. })
        ));
    }

    #[test]
    fn collects_unique_scopes() {
        let mut registry = CapabilityRegistry::default();
        registry
            .register(Arc::new(TestCapability {
                name: "a",
                dependencies: &[],
                scopes: &["a:read", "shared:read"],
                job: None,
                consumer: None,
            }))
            .unwrap();
        registry
            .register(Arc::new(TestCapability {
                name: "b",
                dependencies: &[],
                scopes: &["b:read", "shared:read"],
                job: None,
                consumer: None,
            }))
            .unwrap();
        assert_eq!(registry.scopes(), vec!["a:read", "b:read", "shared:read"]);
    }

    #[test]
    fn rejects_duplicate_job_handlers() {
        let mut registry = CapabilityRegistry::default();
        registry
            .register(Arc::new(TestCapability {
                name: "a",
                dependencies: &[],
                scopes: &[],
                job: Some("shared.job"),
                consumer: None,
            }))
            .unwrap();
        let error = registry
            .register(Arc::new(TestCapability {
                name: "b",
                dependencies: &[],
                scopes: &[],
                job: Some("shared.job"),
                consumer: None,
            }))
            .unwrap_err();
        assert!(matches!(
            error,
            RegistrationError::DuplicateJobHandler("shared.job")
        ));
    }

    #[test]
    fn rejects_duplicate_event_consumers() {
        let mut registry = CapabilityRegistry::default();
        registry
            .register(Arc::new(TestCapability {
                name: "a",
                dependencies: &[],
                scopes: &[],
                job: None,
                consumer: Some("shared.consumer"),
            }))
            .unwrap();
        let error = registry
            .register(Arc::new(TestCapability {
                name: "b",
                dependencies: &[],
                scopes: &[],
                job: None,
                consumer: Some("shared.consumer"),
            }))
            .unwrap_err();
        assert!(matches!(
            error,
            RegistrationError::DuplicateEventConsumer("shared.consumer")
        ));
    }
}
