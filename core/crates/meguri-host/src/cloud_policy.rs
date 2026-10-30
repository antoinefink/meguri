//! Host-borrowed entry points for AppModel's retained Rust policy driver.
use super::CoreHost;
use meguri_sync::cloud::policy::{Command, DeviceEvent, Driver, DriverError};
use std::time::Duration;
impl CoreHost {
    pub fn cloud_policy_driver(&self) -> Option<&Driver> {
        self.cloud_policy.as_ref()
    }
    pub fn cloud_policy_input(&self, event: DeviceEvent) -> Result<Vec<Command>, DriverError> {
        let engine = self.engine();
        self.cloud_policy
            .as_ref()
            .ok_or(DriverError::StaleCommand)?
            .handle(engine.state(), engine.generation(), event)
    }
    pub fn execute_cloud_policy_core(&self, command: &Command, timeout: Duration) -> Result<Vec<Command>, DriverError> {
        let driver = self.cloud_policy.as_ref().ok_or(DriverError::StaleCommand)?;
        if matches!(
            &command.action.kind,
            meguri_sync::cloud::policy::ActionKind::StatusChanged(_)
        ) {
            driver.validate(command)?;
            return Ok(Vec::new());
        }
        let collection = self.collection()?;
        driver.execute_core(command, &collection, self.engine().state(), timeout)
    }
}
