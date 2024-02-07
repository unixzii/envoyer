mod descriptor;
mod handle;
mod manager;
mod stdout_buf;

use stdout_buf::StdoutBuffer;

pub use descriptor::JobDescriptor;
pub use handle::JobHandle;
pub use manager::JobManager;
