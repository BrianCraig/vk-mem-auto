// stub
type MemorySelectorInfo = u64;
type MemorySelection = u64;
pub type MemorySelector = fn(MemorySelectorInfo) -> MemorySelection;

#[derive(Clone)]
pub enum AllocationUsage {
    /// Fastest memory for GPU-only resources. Usually can't be Mapped.
    GpuOnly,
    /// CPU writes, GPU reads. Can be Mapped.
    Upload,
    /// GPU writes, CPU reads. Can be Mapped.
    Readback,
    /// CPU-owned memory. Can be Mapped.
    Cpu,
    /// User-defined memory selection.
    Custom(MemorySelector),
}

#[derive(Clone)]
pub struct AllocationConfig {
    pub(crate) usage: AllocationUsage,
}

impl From<AllocationUsage> for AllocationConfig {
    fn from(usage: AllocationUsage) -> Self {
        AllocationConfig { usage }
    }
}
