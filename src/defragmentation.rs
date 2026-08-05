use std::ptr::null_mut;

use crate::ffi;
use crate::Allocation;
use crate::Allocator;
use ash::prelude::VkResult;
use ash::vk;

pub use ffi::VmaDefragmentationInfo as DefragmentationInfo;
pub use ffi::VmaDefragmentationStats as DefragmentationStats;

pub struct DefragmentationMove {
    pub source: Allocation,
    pub destination: Allocation,
}

impl From<&ffi::VmaDefragmentationMove> for DefragmentationMove {
    fn from(vma_move: &ffi::VmaDefragmentationMove) -> Self {
        Self {
            source: Allocation(vma_move.srcAllocation),
            destination: Allocation(vma_move.dstTmpAllocation),
        }
    }
}

impl Default for DefragmentationInfo {
    fn default() -> Self {
        Self {
            flags: ffi::VmaDefragmentationFlagBits::VMA_DEFRAGMENTATION_FLAG_ALGORITHM_BALANCED_BIT
                as u32,
            pool: null_mut(),
            maxBytesPerPass: 0,
            maxAllocationsPerPass: 0,
            pfnBreakCallback: None,
            pBreakCallbackUserData: null_mut(),
        }
    }
}
pub struct DefragmentationContext<'a> {
    allocator: &'a Allocator,
    raw: ffi::VmaDefragmentationContext,
}

impl<'a> Drop for DefragmentationContext<'a> {
    fn drop(&mut self) {
        unsafe {
            ffi::vmaEndDefragmentation(self.allocator.internal, self.raw, std::ptr::null_mut());
        }
    }
}

impl<'a> DefragmentationContext<'a> {
    /// Ends defragmentation process.
    pub fn end(self) -> DefragmentationStats {
        let mut stats = DefragmentationStats {
            bytesMoved: 0,
            bytesFreed: 0,
            allocationsMoved: 0,
            deviceMemoryBlocksFreed: 0,
        };
        unsafe {
            ffi::vmaEndDefragmentation(self.allocator.internal, self.raw, &mut stats);
        }
        std::mem::forget(self);
        stats
    }

    /// Returns `false` if no more moves are possible or `true` if more defragmentations are possible.
    pub fn begin_pass<F>(&self, mover: F) -> bool
    where
        F: FnOnce(&[DefragmentationMove]),
    {
        let mut pass_info = ffi::VmaDefragmentationPassMoveInfo {
            moveCount: 0,
            pMoves: std::ptr::null_mut(),
        };
        let result = unsafe {
            ffi::vmaBeginDefragmentationPass(self.allocator.internal, self.raw, &mut pass_info)
        };
        if result == vk::Result::SUCCESS {
            return false;
        }
        debug_assert_eq!(result, vk::Result::INCOMPLETE);
        let moves: Vec<_> =
            unsafe { std::slice::from_raw_parts(pass_info.pMoves, pass_info.moveCount as usize) }
                .iter()
                .map(|e| {
                    debug_assert_eq!(
                e.operation,
                ffi::VmaDefragmentationMoveOperation::VMA_DEFRAGMENTATION_MOVE_OPERATION_COPY
            );
                    e
                })
                .map(Into::into)
                .collect();
        mover(&moves);
        let result = unsafe {
            ffi::vmaEndDefragmentationPass(self.allocator.internal, self.raw, &mut pass_info)
        };

        return result == vk::Result::INCOMPLETE;
    }
}

impl Allocator {
    /// Begins defragmentation process.
    ///
    /// ## Returns
    /// `VK_SUCCESS` if defragmentation can begin.
    /// `VK_ERROR_FEATURE_NOT_PRESENT` if defragmentation is not supported.
    pub unsafe fn begin_defragmentation(
        &self,
        info: &ffi::VmaDefragmentationInfo,
    ) -> VkResult<DefragmentationContext<'_>> {
        let mut context: ffi::VmaDefragmentationContext = std::ptr::null_mut();

        ffi::vmaBeginDefragmentation(self.internal, info, &mut context).result()?;

        Ok(DefragmentationContext {
            allocator: self,
            raw: context,
        })
    }
}
#[cfg(test)]
mod test {
    use ash::vk;

    use crate as vk_mem;
    use crate::test_suite::run::TestHarness;
    use crate::{Alloc, Allocation};
    #[test]
    fn defragment_gpu_buffers() {
        let harness = TestHarness::new();
        let allocator = harness.create_allocator();
        let mut buffers = Vec::new();

        let allocate_size = |size: vk::DeviceSize| {
            /* let bci = Box::new(); */
            let bci = vk::BufferCreateInfo::default().size(size).usage(
                vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST
                    | vk::BufferUsageFlags::VERTEX_BUFFER,
            );
            let buffer = unsafe { harness.device.create_buffer(&bci, None).unwrap() };
            let alloc_info = vk_mem::AllocationCreateInfo {
                // user_data: Box::into_raw(bci).cast::<c_void>().addr(),
                ..Default::default()
            };
            let allocation = unsafe {
                allocator
                    .allocate_memory_for_buffer(buffer, &alloc_info)
                    .unwrap()
            };
            let allocation_info = allocator.get_allocation_info(&allocation);
            unsafe {
                harness
                    .device
                    .bind_buffer_memory(
                        buffer,
                        allocation_info.device_memory,
                        allocation_info.offset,
                    )
                    .unwrap()
            }
            let alloc_pointer = allocation.get_raw();
            Some((buffer, alloc_pointer))
        };

        // Allocate many buffers.
        for _ in 0..64 {
            buffers.push(allocate_size(64 * 1024));
        }

        // Free every other allocation.
        for i in 0..(buffers.len() / 2) {
            let (buffer, allocation_handle) = buffers.remove(i).unwrap();
            unsafe {
                allocator.destroy_buffer(buffer, &mut Allocation::from_raw(allocation_handle))
            };
        }

        // Allocate larger buffers to worsen fragmentation.
        for _ in 0..8 {
            buffers.push(allocate_size(256 * 1024));
        }

        let defrag_info = vk_mem::DefragmentationInfo::default();

        let ctx = unsafe { allocator.begin_defragmentation(&defrag_info).unwrap() };

        let mut moved = 0usize;

        while ctx.begin_pass(|moves| {
            for mv in moves {
                moved += 1;
                let source_info = allocator.get_allocation_info(&mv.source);
                let destination_info = allocator.get_allocation_info(&mv.destination);

                let old_buffer = {
                    let index = buffers
                        .iter()
                        .position(|x| x.unwrap().1 == mv.source.get_raw())
                        .unwrap();
                    buffers.remove(index).unwrap().0
                };

                let bci = vk::BufferCreateInfo::default()
                    .size(source_info.size)
                    .usage(
                        vk::BufferUsageFlags::TRANSFER_SRC
                            | vk::BufferUsageFlags::TRANSFER_DST
                            | vk::BufferUsageFlags::VERTEX_BUFFER,
                    );

                let new_buffer = unsafe { harness.device.create_buffer(&bci, None).unwrap() };

                unsafe {
                    harness
                        .device
                        .bind_buffer_memory(
                            new_buffer,
                            destination_info.device_memory,
                            destination_info.offset,
                        )
                        .unwrap()
                }
                unsafe {
                    harness
                        .device
                        .begin_command_buffer(
                            harness.command_buffer,
                            &vk::CommandBufferBeginInfo::default()
                                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                        )
                        .unwrap();
                    let region = vk::BufferCopy {
                        src_offset: 0,
                        dst_offset: 0,
                        size: source_info.size,
                    };
                    harness.device.cmd_copy_buffer(
                        harness.command_buffer,
                        old_buffer,
                        new_buffer,
                        std::slice::from_ref(&region),
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

                unsafe {
                    harness.device.destroy_buffer(old_buffer, None);
                }

                buffers.push(Some((new_buffer, mv.source.get_raw())));
            }
        }) {}

        let stats = ctx.end();
        assert!(moved > 0);
        assert!(stats.allocationsMoved > 0);

        for entry in buffers.into_iter().flatten() {
            let (buffer, allocation) = entry;
            unsafe {
                harness.device.destroy_buffer(buffer, None);
                allocator.free_memory(&mut Allocation::from_raw(allocation));
            }
        }
    }
}
