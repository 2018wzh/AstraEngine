use super::NativeVnHostError;
use astra_runtime::TickIntegrityMode;
use astra_worker_budget::WorkerBudgetBroker;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeVnRuntimeExecution {
    pub integrity_mode: TickIntegrityMode,
    pub worker_count: usize,
}

impl NativeVnRuntimeExecution {
    pub const fn shipping_serial() -> Self {
        Self {
            integrity_mode: TickIntegrityMode::Shipping,
            worker_count: 1,
        }
    }

    pub fn shipping_parallel() -> Self {
        Self::parallel(TickIntegrityMode::Shipping)
    }

    pub fn evidence_parallel() -> Self {
        Self::parallel(TickIntegrityMode::Evidence)
    }

    fn parallel(integrity_mode: TickIntegrityMode) -> Self {
        Self {
            integrity_mode,
            worker_count: WorkerBudgetBroker::global().limit(),
        }
    }

    pub fn validate(&self) -> Result<(), NativeVnHostError> {
        if !(1..=WorkerBudgetBroker::DEFAULT_LIMIT).contains(&self.worker_count) {
            return Err(NativeVnHostError::Input(
                "ASTRA_RUNTIME_EXECUTOR_CONFIG: worker count is outside the Runtime budget".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_execution_uses_the_process_budget_without_abi_conversions() {
        let serial = NativeVnRuntimeExecution::shipping_serial();
        assert_eq!(serial.worker_count, 1);
        serial.validate().unwrap();
        for execution in [
            NativeVnRuntimeExecution::shipping_parallel(),
            NativeVnRuntimeExecution::evidence_parallel(),
        ] {
            assert_eq!(execution.worker_count, WorkerBudgetBroker::global().limit());
            execution.validate().unwrap();
        }
        assert_eq!(
            NativeVnRuntimeExecution::evidence_parallel().integrity_mode,
            TickIntegrityMode::Evidence
        );
    }

    #[test]
    fn package_open_rejects_invalid_worker_counts() {
        let bytes = crate::test_native_package::product_package_with_request(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    text key:hello speaker:hero #@id hello\n", |_| {},
        );
        let package = astra_package::PackageReader::open(&bytes).unwrap();
        for worker_count in [0, WorkerBudgetBroker::DEFAULT_LIMIT + 1, usize::MAX] {
            let result = super::super::NativeVnHostCommandSource::from_package_with_execution(
                &package,
                astra_vn_core::VnRunConfig::classic("en"),
                320,
                180,
                astra_player_core::PlayerHostResourceId(1),
                NativeVnRuntimeExecution {
                    integrity_mode: TickIntegrityMode::Shipping,
                    worker_count,
                },
            );
            let Err(error) = result else {
                panic!("invalid workers opened a session")
            };
            assert!(error.to_string().contains("ASTRA_RUNTIME_EXECUTOR_CONFIG"));
        }
    }
}
