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
