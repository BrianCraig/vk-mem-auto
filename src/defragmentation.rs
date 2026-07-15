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
        let moves: Vec<_> = unsafe {
            std::slice::from_raw_parts(pass_info.pMoves, pass_info.moveCount as usize)
        }
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
