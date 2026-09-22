use crate::{PackageHandle, RuntimeConfig, RuntimeError, RuntimeWorld, TickIntegrityMode};

/// Owns the engine world and its task lifetime independently of any game semantics.
///
/// Products keep their story or rules state beside this session. Dropping the session
/// cancels outstanding world tasks through `RuntimeWorld`'s shutdown lifecycle.
pub struct EngineSession {
    world: RuntimeWorld,
}

impl EngineSession {
    pub fn new(
        config: RuntimeConfig,
        integrity: TickIntegrityMode,
        package: Option<PackageHandle>,
        worker_count: usize,
    ) -> Result<Self, RuntimeError> {
        let mut world = RuntimeWorld::create_with_integrity(config, integrity)?;
        if let Some(package) = package {
            world = world.with_package(package)?;
        }
        world.set_machine_worker_count(worker_count)?;
        Ok(Self { world })
    }

    /// Transfer an already configured embedding world into its session owner.
    pub fn from_world(world: RuntimeWorld) -> Self {
        Self { world }
    }

    pub fn seed(&self) -> u64 {
        self.world.seed()
    }

    pub fn world(&self) -> &RuntimeWorld {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut RuntimeWorld {
        &mut self.world
    }

    /// Validate the next logic step before a product mutates its own state.
    pub fn validate_step(
        &self,
        timing: crate::TickInput,
        mode: crate::TickMode,
    ) -> Result<(), RuntimeError> {
        self.world.validate_tick_request(&crate::TickRequest {
            timing,
            mode,
            ingress: Vec::new(),
        })
    }

    pub fn tick(&mut self, request: crate::TickRequest) -> Result<crate::TickReport, RuntimeError> {
        self.world.tick(request)
    }

    pub fn close(self) {
        drop(self);
    }
}
