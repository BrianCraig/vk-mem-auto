use std::ops::BitOr;

use ash::prelude::VkResult;
use ash::vk::{self, BufferCreateInfo, ImageCreateInfo};

use crate::{Alloc, Allocation, AllocationCreateFlags};

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
pub enum CreateInfo<'a> {
    Buffer(BufferCreateInfo<'a>),
    Image(ImageCreateInfo<'a>),
}

#[derive(Clone)]
pub enum Resource {
    Buffer(vk::Buffer, crate::ffi::VmaAllocation),
    Image(vk::Image, crate::ffi::VmaAllocation),
}

// stub
type MemorySelectorInfo = u64;
type MemorySelection = u64;
pub type MemorySelector = fn(MemorySelectorInfo) -> MemorySelection;

#[derive(Clone)]
pub struct ManagedAllocation<'a> {
    create_info: CreateInfo<'a>,
    usage: AllocationUsage,
    resource: Resource,
}

impl<'a> ManagedAllocation<'a> {
    fn get_vma_alloc(&self) -> crate::ffi::VmaAllocation {
        match self.resource {
            Resource::Buffer(_, pointer) => pointer,
            Resource::Image(_, pointer) => pointer,
        }
    }

    fn get_bci(&self) -> BufferCreateInfo<'_> {
        match self.create_info {
            CreateInfo::Buffer(bci) => bci.clone(),
            _ => panic!(""),
        }
    }
    fn get_ici(&self) -> ImageCreateInfo<'_> {
        match self.create_info {
            CreateInfo::Image(ici) => ici.clone(),
            _ => panic!(""),
        }
    }

    fn get_size(&self) -> u64 {
        match self.create_info {
            CreateInfo::Image(_) => todo!(),
            CreateInfo::Buffer(bci) => bci.size,
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct ManagedAllocationHandle {
    // TODO: add atomic alloc id match to ensure its the same allocator.
    // Maybe should be ManagedBufferHandle and ManagedImageHandle
    index: usize,
    // TODO: add generation and reuse the handle
}

/// Allocator.
///
/// If you need `Send + Sync + Clone`, use [`AllocatorThreadSafe`].
pub struct AllocatorSingleThread<'a> {
    allocator: crate::Allocator,
    device: &'a ash::Device,
    managed_allocations: Vec<Option<ManagedAllocation<'a>>>,
    queue: ash::vk::Queue,
    command_buffer: ash::vk::CommandBuffer,
    fence: ash::vk::Fence,
}

impl<'a> AllocatorSingleThread<'a> {
    pub fn new(
        instance: &'a ash::Instance,
        device: &'a ash::Device,
        physical_device: ash::vk::PhysicalDevice,
        queue: ash::vk::Queue,
        command_pool: ash::vk::CommandPool,
    ) -> Self {
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
            device,
            managed_allocations: vec![],
            queue,
            command_buffer,
            fence,
        }
    }

    pub fn allocate(
        &mut self,
        create_info: CreateInfo<'a>,
        usage: AllocationUsage,
    ) -> VkResult<ManagedAllocationHandle> {
        // TODO: use first available
        let index = self.managed_allocations.len();
        let ci_with_flags =
            match create_info {
                CreateInfo::Buffer(buffer_create_info) => {
                    CreateInfo::Buffer(buffer_create_info.usage(buffer_create_info.usage.bitor(
                        vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST,
                    )))
                }
                CreateInfo::Image(image_create_info) => {
                    CreateInfo::Image(image_create_info.usage(image_create_info.usage.bitor(
                        vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST,
                    )))
                }
            };

        let resource = match &ci_with_flags {
            CreateInfo::Buffer(buffer_create_info) => {
                let buffer =
                    unsafe { self.device.create_buffer(buffer_create_info, None).unwrap() };
                let allocation = unsafe {
                    self.allocator
                        .allocate_memory_for_buffer(
                            buffer,
                            &crate::AllocationCreateInfo {
                                flags: AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE,
                                required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                                ..Default::default()
                            },
                        )
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
                }
                Resource::Buffer(buffer, allocation.get_raw())
            }
            CreateInfo::Image(image_create_info) => {
                let image = unsafe { self.device.create_image(image_create_info, None).unwrap() };
                let allocation = unsafe {
                    self.allocator
                        .allocate_memory_for_image(image, &crate::AllocationCreateInfo::default())
                        .unwrap()
                };
                let allocation_info = self.allocator.get_allocation_info(&allocation);
                unsafe {
                    self.device
                        .bind_image_memory(
                            image,
                            allocation_info.device_memory,
                            allocation_info.offset,
                        )
                        .unwrap()
                }
                Resource::Image(image, allocation.get_raw())
            }
        };
        self.managed_allocations.push(Some(ManagedAllocation {
            create_info: ci_with_flags,
            usage: usage,
            resource,
        }));

        Ok(ManagedAllocationHandle { index })
    }

    /// Destroys the Buffer/Image associated with it.
    ///
    /// You must ensure that the Resource associated with it is not being used
    pub unsafe fn free(&mut self, handle: ManagedAllocationHandle) -> VkResult<()> {
        self.managed_allocations
            .get(handle.index)
            .expect("index should be between bounds")
            .as_ref()
            .expect("Alloc is destroyed");
        // TODO: do checks of generation and allocator id.
        match &(self.managed_allocations[handle.index]
            .take()
            .unwrap()
            .resource)
        {
            Resource::Buffer(buffer, allocation) => {
                self.device.destroy_buffer(*buffer, None);
                self.allocator
                    .free_memory(&mut crate::Allocation::from_raw(*allocation));
            }
            Resource::Image(image, allocation) => {
                self.device.destroy_image(*image, None);
                self.allocator
                    .free_memory(&mut crate::Allocation::from_raw(*allocation));
            }
        }
        Ok(())
    }

    pub fn get_buffer(&self, handle: ManagedAllocationHandle) -> VkResult<vk::Buffer> {
        todo!()
    }

    pub fn get_image(&self, handle: ManagedAllocationHandle) -> VkResult<vk::Image> {
        todo!()
    }

    /// Returns the mapped memory of the resource. Drop it when unused.
    ///
    /// Ensure all returned instances are dropped before freeing or defragging this resource.
    pub fn map(&self, handle: ManagedAllocationHandle) -> VkResult<OwnedMap<'_>> {
        let alloc = self
            .managed_allocations
            .get(handle.index)
            .expect("index should be between bounds")
            .as_ref()
            .expect("Alloc is destroyed")
            .get_vma_alloc();
        let mut crate_alloc = unsafe { Allocation::from_raw(alloc) };
        let pointer = unsafe { self.allocator.map_memory(&mut crate_alloc) }.unwrap();

        //self.allocator.unmap_memory(allocation);
        //self.allocator.map_memory(allocation)
        Ok(OwnedMap {
            device: self.device,
            allocator: &self.allocator,
            allocation: crate_alloc.0,
            pointer,
        })
    }

    /// Defrags all allocated memory.
    /// You should call it whenever the resources allocated are not being used, for example between frames.
    ///
    /// You **must** ensure that all the movable resources are not being used, since destroying a
    /// resource (buffer/image) in vulkan while being used is UB.
    pub unsafe fn defrag(&mut self) {
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

                let index = self
                    .managed_allocations
                    .iter()
                    .position(|x: &Option<ManagedAllocation<'_>>| {
                        x.as_ref()
                            .is_some_and(|x| x.get_vma_alloc() == mv.source.get_raw())
                    })
                    .unwrap();
                let mut man_alloc = self
                    .managed_allocations
                    .get(index)
                    .unwrap()
                    .clone()
                    .take()
                    .unwrap();
                man_alloc.resource = match &man_alloc.resource {
                    Resource::Buffer(buffer, alloc) => Resource::Buffer(
                        unsafe {
                            destroy_buffers.push(*buffer);
                            let new_buffer = self
                                .device
                                .create_buffer(&man_alloc.get_bci(), None)
                                .unwrap();
                            self.device
                                .bind_buffer_memory(
                                    new_buffer,
                                    destination_info.device_memory,
                                    destination_info.offset,
                                )
                                .unwrap();
                            self.device.cmd_copy_buffer(
                                self.command_buffer,
                                *buffer,
                                new_buffer,
                                std::slice::from_ref(&vk::BufferCopy {
                                    src_offset: 0,
                                    dst_offset: 0,
                                    size: source_info.size,
                                }),
                            );
                            new_buffer
                        },
                        *alloc,
                    ),
                    Resource::Image(image, alloc) => Resource::Image(
                        unsafe {
                            destroy_images.push(*image);
                            let new_image = self
                                .device
                                .create_image(&man_alloc.get_ici(), None)
                                .unwrap();
                            self.device
                                .bind_image_memory(
                                    new_image,
                                    destination_info.device_memory,
                                    destination_info.offset,
                                )
                                .unwrap();
                            let ici = man_alloc.get_ici();
                            self.device.cmd_copy_image(
                                self.command_buffer,
                                *image,
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
                        *alloc,
                    ),
                };

                self.managed_allocations[index] = Some(man_alloc);
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

    pub fn get_device_memory_and_offset(
        &self,
        ManagedAllocationHandle { index }: ManagedAllocationHandle,
    ) -> (vk::DeviceMemory, u64) {
        let alloc = self
            .managed_allocations
            .get(index)
            .expect("index should be between bounds")
            .as_ref()
            .expect("Alloc is destroyed")
            .get_vma_alloc();
        let info = self
            .allocator
            .get_allocation_info(&unsafe { Allocation::from_raw(alloc) });
        (info.device_memory, info.offset)
    }
}

impl<'a> Drop for AllocatorSingleThread<'a> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_fence(self.fence, None);
        }
    }
}

pub struct OwnedMap<'a> {
    device: &'a ash::Device,
    allocator: &'a crate::Allocator,
    allocation: crate::ffi::VmaAllocation,
    pointer: *mut u8,
}

impl<'a> OwnedMap<'a> {
    pub fn pointer(&self) -> *mut u8 {
        self.pointer
    }

    /// Flushes the whole size of the mapped resource.
    /// 
    /// TODO: This should check if HOST_COHERENT and skip it.
    pub fn flush(&self) {
        let crate::AllocationInfo {
            device_memory,
            offset,
            ..
        } = self
            .allocator
            .get_allocation_info(&unsafe { crate::Allocation::from_raw(self.allocation) });

        let memory_range = vk::MappedMemoryRange::default()
            .memory(device_memory)
            .offset(offset)
            .size(vk::WHOLE_SIZE);

        unsafe {
            self.device
                .flush_mapped_memory_ranges(&[memory_range])
                .expect("Failed to flush mapped memory ranges.");
        }
    }
}

impl<'a> Drop for OwnedMap<'a> {
    fn drop(&mut self) {
        unsafe {
            self.allocator
                .unmap_memory(&mut crate::Allocation::from_raw(self.allocation))
        };
    }
}