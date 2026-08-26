pub mod abi;
pub mod concurrent_runtime_host;
pub mod descriptor;
#[cfg(feature = "dynamic-abi")]
pub mod loader;
pub mod registry;
pub mod runtime_host;
pub mod session_batch;

pub use abi::*;
pub use astra_runtime::{
    ActionInvocation, ActionTrace, BlackboardValue, EventPayload, EventSource, PresentationCommand,
};
pub use astra_worker_budget::{WorkerBudgetBroker, WorkerBudgetLease};
pub use concurrent_runtime_host::{
    ConcurrentProductRuntimeHost, ConcurrentProductRuntimeHost as ProductRuntimeHostV2,
    ProductRuntimeProviderFactory, ProductRuntimeSession,
};
pub use descriptor::*;
#[cfg(feature = "dynamic-abi")]
pub use loader::*;
pub use registry::*;
// v1 同步宿主（legacy）：保留以兼容存量调用方，新代码请使用 ProductRuntimeHostV2
pub use runtime_host::{
    AsyncProductRuntimeHost, ProductRuntimeHost, ProductRuntimeHost as ProductRuntimeHostV1,
    ProductRuntimeProvider, RuntimeHostError, RuntimeHostLimits,
};
pub use session_batch::*;
