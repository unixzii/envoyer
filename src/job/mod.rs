mod descriptor;
mod handle;
mod manager;
mod stdout_buf;

pub use descriptor::JobDescriptor;
pub use handle::JobHandle;
pub use manager::JobManager;
use stdout_buf::StdoutBuffer;
