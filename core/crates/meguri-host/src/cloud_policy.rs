//! Host-borrowed entry points for AppModel's retained Rust policy driver.
use super::CoreHost;
use meguri_sync::cloud::policy::{Command, DeviceEvent, Driver, DriverError, PolicyWork};
use std::time::Duration;
impl CoreHost {
    pub fn cloud_policy_driver(&self) -> Option<&Driver> {
        self.cloud_policy.as_ref()
    }
    pub fn cloud_policy_input(&self, event: DeviceEvent) -> Result<Vec<Command>, DriverError> {
        self.cloud_policy_input_with_work(event, &mut PolicyWork::default())
    }
    pub fn cloud_policy_input_with_work(
        &self,
        event: DeviceEvent,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let engine = self.engine();
        self.cloud_policy
            .as_ref()
            .ok_or(DriverError::StaleCommand)?
            .handle_with_work(engine.state(), engine.generation(), event, work)
    }
    pub fn execute_cloud_policy_core(&self, command: &Command, timeout: Duration) -> Result<Vec<Command>, DriverError> {
        self.execute_cloud_policy_core_with_work(command, timeout, &mut PolicyWork::default())
    }
    pub fn execute_cloud_policy_core_with_work(
        &self,
        command: &Command,
        timeout: Duration,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, DriverError> {
        let driver = self.cloud_policy.as_ref().ok_or(DriverError::StaleCommand)?;
        if matches!(
            &command.action.kind,
            meguri_sync::cloud::policy::ActionKind::StatusChanged(_)
        ) {
            driver.validate_with_work(command, work)?;
            return Ok(Vec::new());
        }
        let collection = self.collection()?;
        driver.execute_core_with_work(command, &collection, self.engine().state(), timeout, work)
    }
    /// Detailed original pre-effect refusal; all acquisition/owner failures remain plain.
    pub fn execute_cloud_policy_core_attempt_with_work(
        &self,
        command: &Command,
        timeout: Duration,
        work: &mut PolicyWork,
    ) -> Result<Vec<Command>, meguri_sync::cloud::policy::CommandAttemptFailure> {
        let driver = self.cloud_policy.as_ref().ok_or(DriverError::StaleCommand)?;
        if matches!(
            &command.action.kind,
            meguri_sync::cloud::policy::ActionKind::StatusChanged(_)
        ) {
            driver.validate_with_work(command, work)?;
            return Ok(Vec::new());
        }
        let collection = self.collection().map_err(DriverError::from)?;
        driver.execute_core_attempt_with_work(command, &collection, self.engine().state(), timeout, work)
    }
}
