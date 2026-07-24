mod ash_owned;

use std::cell::RefCell;
use std::ops::{BitOr, Deref};
use std::rc::Rc;

use ash::prelude::VkResult;
use ash::vk::{self, BufferCreateInfo, ImageCreateInfo};

use crate::{Alloc, Allocation, AllocationCreateFlags, AllocationCreateInfo};

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
pub enum Resource {
    Buffer(
        vk::Buffer,
        crate::ffi::VmaAllocation,
        ash_owned::BufferCreateInfoOwned,
    ),
    Image(
        vk::Image,
        crate::ffi::VmaAllocation,
        ash_owned::ImageCreateInfoOwned,
    ),
}

// stub
type MemorySelectorInfo = u64;
type MemorySelection = u64;
pub type MemorySelector = fn(MemorySelectorInfo) -> MemorySelection;

#[derive(Clone)]
pub struct ManagedAllocation {
    usage: AllocationUsage,
    resource: Resource,
    size: vk::DeviceSize,
    mem_offset: (vk::DeviceMemory, vk::DeviceSize),
    freed: bool,
}

impl ManagedAllocation {
    fn get_vma_alloc(&self) -> crate::ffi::VmaAllocation {
        match self.resource {
            Resource::Buffer(_, pointer, _) => pointer,
            Resource::Image(_, pointer, _) => pointer,
        }
    }

    fn get_bci<'a>(&self) -> BufferCreateInfo<'a> {
        match self.resource {
            Resource::Buffer(_, _, bcio) => (&bcio).into(),
            Resource::Image(_, _, _) => panic!(),
        }
    }
    fn get_ici<'a>(&self) -> ImageCreateInfo<'a> {
        match self.resource {
            Resource::Buffer(_, _, _) => panic!(),
            Resource::Image(_, _, icio) => (&icio).into(),
        }
    }
}

#[derive(Clone)]
pub struct AllocatorHandle(Rc<RefCell<AllocatorSingleThread>>);

impl Deref for AllocatorHandle {
    type Target = RefCell<AllocatorSingleThread>;

    fn deref<'a>(&self) -> &Self::Target {
        &self.0
    }
}

impl From<AllocatorSingleThread> for AllocatorHandle {
    fn from(inner: AllocatorSingleThread) -> Self {
        AllocatorHandle(Rc::new(RefCell::new(inner)))
    }
}

impl AllocatorHandle {
    pub fn allocate_buffer(
        &self,
        buffer_create_info: vk::BufferCreateInfo<'_>,
        usage: AllocationUsage,
    ) -> VkResult<ManagedAllocationHandle> {
        self.0
            .borrow_mut()
            .allocate_buffer(buffer_create_info, usage, self.clone())
    }

    pub fn allocate_image(
        &self,
        image_create_info: vk::ImageCreateInfo<'_>,
        usage: AllocationUsage,
    ) -> VkResult<ManagedAllocationHandle> {
        self.0
            .borrow_mut()
            .allocate_image(image_create_info, usage, self.clone())
    }

    pub unsafe fn defrag(&mut self) {
        self.0.borrow_mut().defrag()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandleError {
    FreedResource,
    DroppedAllocator,
}

type RcManagedAllocation = Rc<RefCell<ManagedAllocation>>;

#[derive(Clone)]
pub struct ManagedAllocationHandle {
    allocator: AllocatorHandle,
    rc_ma: RcManagedAllocation,
}

impl ManagedAllocationHandle {
    pub(crate) fn inner(&self) -> Result<&ManagedAllocation, HandleError> {
        Err(HandleError::FreedResource)
    }

    pub fn size(&self) -> Result<u64, HandleError> {
        let ma = self.rc_ma.borrow();
        match ma.freed {
            true => Err(HandleError::FreedResource),
            false => Ok(ma.size),
        }
    }

    pub unsafe fn free(&self) -> Result<(), HandleError> {
        unsafe { self.allocator.borrow_mut().free(&self.rc_ma) }
    }

    pub fn map<F>(&self, f: F) -> Result<(), HandleError>
    where
        F: FnOnce(*mut u8, &dyn Fn()),
    {
        self.allocator.borrow().map(&self.rc_ma, f)
    }
}

impl Drop for ManagedAllocationHandle {
    fn drop(&mut self) {
        // If we are dropping this, this rc_ma and the one on the allocator still exists
        // That means that this last "user available" handle goes out of scope.
        // We must free the resource (if still not, checked by the allocator)
        if Rc::strong_count(&self.rc_ma) == 2 {
            let _ = unsafe { Self::free(&self) };
        } else {}
    }
}

/// Allocator. Thread-Safe not in scope still.
pub struct AllocatorSingleThread {
    allocator: crate::Allocator,
    device: ash::Device,
    managed_allocations: Vec<RcManagedAllocation>,
    queue: ash::vk::Queue,
    command_buffer: ash::vk::CommandBuffer,
    fence: ash::vk::Fence,
}

impl AllocatorSingleThread {
    pub fn new<'a>(
        instance: &'a ash::Instance,
        device: &'a ash::Device,
        physical_device: ash::vk::PhysicalDevice,
        queue: ash::vk::Queue,
        command_pool: ash::vk::CommandPool,
    ) -> AllocatorHandle {
        let create_info = crate::AllocatorCreateInfo::new(instance, device, physical_device);
        let allocator = unsafe { crate::Allocator::new(create_info).unwrap() };
        let command_buffer = unsafe {
            device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(command_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .unwrap()[0]
        };
        let fence = unsafe {
            device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .unwrap()
        };

        Self {
            allocator,
            device: (*device).clone(),
            managed_allocations: vec![],
            queue,
            command_buffer,
            fence,
        }
        .into()
    }

    fn aci(usage: &AllocationUsage) -> AllocationCreateInfo {
        match usage {
            AllocationUsage::GpuOnly => crate::AllocationCreateInfo {
                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                ..Default::default()
            },
            AllocationUsage::Upload => crate::AllocationCreateInfo {
                flags: AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE,
                required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                ..Default::default()
            },
            AllocationUsage::Readback => crate::AllocationCreateInfo {
                flags: AllocationCreateFlags::HOST_ACCESS_RANDOM,
                required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_CACHED,
                ..Default::default()
            },
            AllocationUsage::Cpu => crate::AllocationCreateInfo {
                flags: AllocationCreateFlags::HOST_ACCESS_RANDOM,
                required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_COHERENT
                    | vk::MemoryPropertyFlags::HOST_CACHED,
                ..Default::default()
            },
            AllocationUsage::Custom(_) => todo!(),
        }
    }

    pub fn allocate_buffer(
        &mut self,
        buffer_create_info: vk::BufferCreateInfo<'_>,
        usage: AllocationUsage,
        allocator_handle: AllocatorHandle,
    ) -> VkResult<ManagedAllocationHandle> {
        let aci = Self::aci(&usage);
        let buffer_create_info = buffer_create_info.usage(
            buffer_create_info
                .usage
                .bitor(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST),
        );
        let buffer_create_info_owned = buffer_create_info.try_into().unwrap();

        let buffer = unsafe {
            self.device
                .create_buffer(&buffer_create_info, None)
                .unwrap()
        };
        let allocation = unsafe {
            self.allocator
                .allocate_memory_for_buffer(buffer, &aci)
                .unwrap()
        };
        let allocation_info = self.allocator.get_allocation_info(&allocation);
        unsafe {
            self.device
                .bind_buffer_memory(
                    buffer,
                    allocation_info.device_memory,
                    allocation_info.offset,
                )
                .unwrap()
        };

        let rc_ma = Rc::new(RefCell::new(ManagedAllocation {
            usage: usage,
            resource: Resource::Buffer(buffer, allocation.get_raw(), buffer_create_info_owned),
            size: allocation_info.size,
            mem_offset: (allocation_info.device_memory, allocation_info.offset),
            freed: false,
        }));

        self.managed_allocations.push(rc_ma.clone());

        Ok(ManagedAllocationHandle {
            allocator: allocator_handle,
            rc_ma,
        })
    }

    pub fn allocate_image(
        &mut self,
        image_create_info: vk::ImageCreateInfo<'_>,
        usage: AllocationUsage,

        allocator_handle: AllocatorHandle,
    ) -> VkResult<ManagedAllocationHandle> {
        let aci = Self::aci(&usage);
        let image_create_info = image_create_info.usage(
            image_create_info
                .usage
                .bitor(vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST),
        );
        let image_create_info_owned = image_create_info.try_into().unwrap();

        let image = unsafe { self.device.create_image(&image_create_info, None).unwrap() };
        let allocation = unsafe {
            self.allocator
                .allocate_memory_for_image(image, &aci)
                .unwrap()
        };
        let allocation_info = self.allocator.get_allocation_info(&allocation);
        unsafe {
            self.device
                .bind_image_memory(image, allocation_info.device_memory, allocation_info.offset)
                .unwrap()
        };

        let rc_ma = Rc::new(RefCell::new(ManagedAllocation {
            usage: usage,
            resource: Resource::Image(image, allocation.get_raw(), image_create_info_owned),
            size: allocation_info.size,
            mem_offset: (allocation_info.device_memory, allocation_info.offset),
            freed: false,
        }));

        self.managed_allocations.push(rc_ma.clone());

        Ok(ManagedAllocationHandle {
            allocator: allocator_handle,
            rc_ma,
        })
    }

    /// Destroys the Buffer/Image associated with it.
    ///
    /// You must ensure that the Resource associated with it is not being used
    unsafe fn free(&mut self, rc_ma: &RcManagedAllocation) -> Result<(), HandleError> {
        let mut ma = rc_ma.borrow_mut();
        if ma.freed {
            return Err(HandleError::FreedResource);
        }
        match &ma.resource {
            Resource::Buffer(buffer, allocation, _) => {
                self.device.destroy_buffer(*buffer, None);
                self.allocator
                    .free_memory(&mut crate::Allocation::from_raw(*allocation));
            }
            Resource::Image(image, allocation, _) => {
                self.device.destroy_image(*image, None);
                self.allocator
                    .free_memory(&mut crate::Allocation::from_raw(*allocation));
            }
        }
        ma.freed = true;
        Ok(())
    }

    /// Why from a handle? Implementation should be in ManagedAllocation(&self), handle must only search it in alloc and call.
    pub fn get_buffer(&self, handle: ManagedAllocationHandle) -> VkResult<vk::Buffer> {
        todo!()
    }

    pub fn get_image(&self, handle: ManagedAllocationHandle) -> VkResult<vk::Image> {
        todo!()
    }

    fn map<F>(&self, rc_ma: &RcManagedAllocation, f: F) -> Result<(), HandleError>
    where
        F: FnOnce(*mut u8, &dyn Fn()),
    {
        let ma = rc_ma.borrow();
        if ma.freed {
            return Err(HandleError::FreedResource);
        }
        let mut crate_alloc = unsafe { Allocation::from_raw(ma.get_vma_alloc()) };
        let pointer = unsafe { self.allocator.map_memory(&mut crate_alloc) }.unwrap();
        let crate::AllocationInfo {
            device_memory,
            offset,
            ..
        } = self.allocator.get_allocation_info(&crate_alloc);
        let flush = || {
            let memory_range = vk::MappedMemoryRange::default()
                .memory(device_memory)
                .offset(offset)
                .size(vk::WHOLE_SIZE);

            unsafe {
                self.device
                    .flush_mapped_memory_ranges(&[memory_range])
                    .expect("Failed to flush mapped memory ranges.");
            }
        };
        f(pointer, &flush);
        unsafe {
            self.allocator.unmap_memory(&mut crate_alloc);
        };
        Ok(())
    }

    /// Defrags all allocated memory.
    /// You should call it whenever the resources allocated are not being used, for example between frames.
    ///
    /// You **must** ensure that all the movable resources are not being used, since destroying a
    /// resource (buffer/image) in vulkan while being used is UB.
    pub unsafe fn defrag(&mut self) {
        self.managed_allocations
            .retain(|rc_ma| !rc_ma.borrow().freed);

        let ctx = unsafe {
            self.allocator
                .begin_defragmentation(&crate::DefragmentationInfo::default())
                .unwrap()
        };
        while ctx.begin_pass(|moves| {
            let mut destroy_buffers = vec![];
            let mut destroy_images = vec![];
            self.device
                .begin_command_buffer(
                    self.command_buffer,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .unwrap();
            for mv in moves {
                let source_info = self.allocator.get_allocation_info(&mv.source);
                let destination_info = self.allocator.get_allocation_info(&mv.destination);

                let rc_ma = self
                    .managed_allocations
                    .iter()
                    .find(|x| x.borrow().get_vma_alloc() == mv.source.get_raw())
                    .expect("vk_mem_auto U01");

                let mut ma = rc_ma.borrow_mut();
                ma.resource = match ma.resource {
                    Resource::Buffer(buffer, alloc, bcio) => Resource::Buffer(
                        unsafe {
                            destroy_buffers.push(buffer);
                            let bci = (&bcio).into();
                            let new_buffer = self.device.create_buffer(&bci, None).unwrap();
                            self.device
                                .bind_buffer_memory(
                                    new_buffer,
                                    destination_info.device_memory,
                                    destination_info.offset,
                                )
                                .unwrap();
                            self.device.cmd_copy_buffer(
                                self.command_buffer,
                                buffer,
                                new_buffer,
                                std::slice::from_ref(&vk::BufferCopy {
                                    src_offset: 0,
                                    dst_offset: 0,
                                    size: source_info.size,
                                }),
                            );
                            new_buffer
                        },
                        alloc,
                        bcio,
                    ),
                    Resource::Image(image, alloc, icio) => Resource::Image(
                        unsafe {
                            destroy_images.push(image);
                            let ici = (&icio).into();
                            let new_image = self.device.create_image(&ici, None).unwrap();
                            self.device
                                .bind_image_memory(
                                    new_image,
                                    destination_info.device_memory,
                                    destination_info.offset,
                                )
                                .unwrap();
                            self.device.cmd_copy_image(
                                self.command_buffer,
                                image,
                                ash::vk::ImageLayout::TRANSFER_SRC_OPTIMAL, // TODO: We need to track the layouts
                                new_image,
                                ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                std::slice::from_ref(&ash::vk::ImageCopy {
                                    src_subresource: ash::vk::ImageSubresourceLayers {
                                        aspect_mask: vk::ImageAspectFlags::COLOR, // TODO: Derive from ici format, test only Color
                                        mip_level: 0,
                                        base_array_layer: 0,
                                        layer_count: ici.array_layers,
                                    },
                                    src_offset: ash::vk::Offset3D { x: 0, y: 0, z: 0 },
                                    dst_subresource: ash::vk::ImageSubresourceLayers {
                                        aspect_mask: vk::ImageAspectFlags::COLOR, // TODO: Derive from ici format, test only Color
                                        mip_level: 0,
                                        base_array_layer: 0,
                                        layer_count: ici.array_layers,
                                    },
                                    dst_offset: ash::vk::Offset3D { x: 0, y: 0, z: 0 },
                                    extent: ash::vk::Extent3D {
                                        width: ici.extent.width,
                                        height: ici.extent.height,
                                        depth: ici.extent.depth,
                                    },
                                }),
                            );
                            new_image
                        },
                        alloc,
                        icio,
                    ),
                };
            }
            unsafe {
                self.device.end_command_buffer(self.command_buffer).unwrap();
                self.device
                    .queue_submit(
                        self.queue,
                        &[vk::SubmitInfo::default()
                            .command_buffers(std::slice::from_ref(&self.command_buffer))],
                        self.fence,
                    )
                    .unwrap();
                self.device
                    .wait_for_fences(&[self.fence], true, 1_000_000_000 * 60) // CUSTOM_NEEDED
                    .unwrap();
                self.device.reset_fences(&[self.fence]).unwrap();
                for buffer in destroy_buffers {
                    self.device.destroy_buffer(buffer, None);
                }
                for image in destroy_images {
                    self.device.destroy_image(image, None);
                }
            }
        }) {}
    }
}

impl Drop for AllocatorSingleThread {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_fence(self.fence, None);
        }
    }
}
