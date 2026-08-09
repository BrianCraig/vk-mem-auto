use crate as vk_mem;

use ash::vk;
use vk_mem::ManagedAllocationHandle;

macro_rules! time {
    ($name:expr, $body:expr) => {{
        let start = std::time::Instant::now();
        let result = $body;
        println!("{} took {} us", $name, start.elapsed().as_micros());
        result
    }};
}

pub(crate) use time;

pub(crate) fn transition_image(
    harness: &crate::test_suite::run::TestHarness,
    handle: &ManagedAllocationHandle,
    new_layout: vk::ImageLayout,
) {
    unsafe {
        harness
            .device
            .begin_command_buffer(
                harness.command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .unwrap();
        let barrier = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .old_layout(handle.current_layout().unwrap())
            .new_layout(new_layout)
            .image(handle.get_image().unwrap())
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(vk::REMAINING_MIP_LEVELS)
                    .layer_count(vk::REMAINING_ARRAY_LAYERS),
            );
        harness.device.cmd_pipeline_barrier(
            harness.command_buffer,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
        harness
            .device
            .end_command_buffer(harness.command_buffer)
            .unwrap();
        harness
            .device
            .queue_submit(
                harness.queue,
                &[vk::SubmitInfo::default()
                    .command_buffers(std::slice::from_ref(&harness.command_buffer))],
                harness.fence,
            )
            .unwrap();
        harness
            .device
            .wait_for_fences(&[harness.fence], true, 1000000000)
            .unwrap();
        harness.device.reset_fences(&[harness.fence]).unwrap();
    }
    handle.set_layout(new_layout).unwrap();
}

pub(crate) fn fill<T: Sized + Clone>(handle: &ManagedAllocationHandle, value: T) {
    let size = handle.size().unwrap() as usize;
    let _ = handle.map(|pointer, flush| unsafe {
        std::slice::from_raw_parts_mut(pointer.cast::<T>(), size / size_of::<T>()).fill(value);
        flush();
    });
}

pub(crate) fn assert_filled_with<T: Sized + PartialEq + Clone>(
    handle: &ManagedAllocationHandle,
    value: T,
) {
    let size = handle.size().unwrap() as usize;
    let count = size / size_of::<T>();
    let _ = handle.map(|pointer, _| unsafe {
        assert!(std::slice::from_raw_parts(pointer.cast::<T>(), count)
            .to_vec()
            .iter()
            .all(|e| *e == value));
    });
}

/// Allocate between other images, then drop both so on defrag there is commonly a move op.
pub(crate) fn allocate_image_sandwiched_drop(
    allocator: &crate::AllocatorHandle,
    image_create_info: vk::ImageCreateInfo<'_>,
    config: impl Into<crate::AllocationConfig>,
) -> crate::VkResult<crate::ManagedAllocationHandle> {
    let _before = allocator
        .allocate_image(
            super::constructors::ica_linear_1024_1024_rgba8(),
            vk_mem::AllocationUsage::Readback,
        )
        .unwrap();
    let out = allocator.allocate_image(image_create_info, config);
    let _after = allocator
        .allocate_image(
            super::constructors::ica_linear_1024_1024_rgba8(),
            vk_mem::AllocationUsage::Readback,
        )
        .unwrap();
    out
}

pub(crate) struct MoveChecker {
    handle: ManagedAllocationHandle,
    mem_offset: (vk::DeviceMemory, u64),
}

impl MoveChecker {
    pub(crate) fn new(handle: &ManagedAllocationHandle) -> MoveChecker {
        MoveChecker {
            handle: handle.clone(),
            mem_offset: handle.rc_ma.borrow().mem_offset,
        }
    }

    /// Assert that the mem has moved.
    ///
    /// This is currently not 100% exact, there is a small chance that the mem is in the
    /// same exact vkMemory and offset, and has moved.
    /// Since this is for testing purpose, we currently don't care too much,
    /// but once we have done the `AbsorbVMA` task, we should attack this problem.
    pub(crate) fn assert_moved(&mut self) {
        let current = self.handle.rc_ma.borrow().mem_offset;
        assert_ne!(self.mem_offset, current, "Allocation has not moved");
        self.mem_offset = current;
    }

    /// Assert that the mem has **not** moved.
    ///
    /// This is currently not 100% exact, there is a small chance that the mem is in the
    /// same exact vkMemory and offset, and has moved.
    /// Since this is for testing purpose, we currently don't care too much,
    /// but once we have done the `AbsorbVMA` task, we should attack this problem.
    ///
    /// please remove `_` on first use.
    pub(crate) fn _assert_not_moved(&self) {
        let current = self.handle.rc_ma.borrow().mem_offset;
        assert_eq!(self.mem_offset, current, "Allocation has moved");
    }
}
