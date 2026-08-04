mod ash_owned;
mod config;

use ash::khr::{dedicated_allocation, get_memory_requirements2};
use bitflags::bitflags;

use std::cell::RefCell;
use std::ffi::CStr;
use std::ops::{BitOr, Deref};
use std::rc::Rc;

use ash::prelude::VkResult;
use ash::vk::{self, ImageCreateInfo};

use self::config::AllocationConfig;
pub use self::config::AllocationUsage;

use crate::{
    Alloc, Allocation, AllocationCreateFlags, AllocationCreateInfo, DefragmentationStats,
    ResourceRequirementHints,
};

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
        vk::ImageLayout,
    ),
}

pub struct ManagedAllocation {
    _config: AllocationConfig,
    resource: Resource,
    hints: ResourceRequirementHints,
    mem_offset: (vk::DeviceMemory, vk::DeviceSize),
    freed: bool,
}

impl ManagedAllocation {
    fn get_vma_alloc(&self) -> crate::ffi::VmaAllocation {
        match self.resource {
            Resource::Buffer(_, pointer, _) => pointer,
            Resource::Image(_, pointer, _, _) => pointer,
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
        config: impl Into<AllocationConfig>,
    ) -> VkResult<ManagedAllocationHandle> {
        self.0
            .borrow_mut()
            .allocate_buffer(buffer_create_info, config.into(), self.clone())
    }

    pub fn allocate_image(
        &self,
        image_create_info: vk::ImageCreateInfo<'_>,
        config: impl Into<AllocationConfig>,
    ) -> VkResult<ManagedAllocationHandle> {
        self.0
            .borrow_mut()
            .allocate_image(image_create_info, config.into(), self.clone())
    }

    pub unsafe fn defrag(&mut self) -> DefragmentationStats {
        self.0.borrow_mut().defrag()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandleError {
    FreedResource,
    IncorrectResourceType,
}

type RcManagedAllocation = Rc<RefCell<ManagedAllocation>>;

#[derive(Clone)]
pub struct ManagedAllocationHandle {
    allocator: AllocatorHandle,
    rc_ma: RcManagedAllocation,
}

impl ManagedAllocationHandle {
    pub fn get_image(&self) -> Result<vk::Image, HandleError> {
        let ma = self.rc_ma.borrow();
        if ma.freed {
            return Err(HandleError::FreedResource);
        }
        match &ma.resource {
            Resource::Buffer(_, _, _) => Err(HandleError::IncorrectResourceType),
            Resource::Image(image, _, _, _) => Ok(*image),
        }
    }

    pub fn size(&self) -> Result<u64, HandleError> {
        let ma = self.rc_ma.borrow();
        match ma.freed {
            true => Err(HandleError::FreedResource),
            false => Ok(ma.hints.size),
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

    pub fn resource_hints(&self) -> ResourceRequirementHints {
        self.rc_ma.borrow().hints
    }

    pub fn current_layout(&self) -> Result<vk::ImageLayout, HandleError> {
        let ma = self.rc_ma.borrow();
        if ma.freed {
            return Err(HandleError::FreedResource);
        }
        match &ma.resource {
            Resource::Buffer(_, _, _) => Err(HandleError::IncorrectResourceType),
            Resource::Image(_, _, _, layout) => Ok(*layout),
        }
    }

    pub fn set_layout(&self, new_layout: vk::ImageLayout) -> Result<(), HandleError> {
        let mut ma = self.rc_ma.borrow_mut();
        if ma.freed {
            return Err(HandleError::FreedResource);
        }
        match &ma.resource {
            Resource::Buffer(_, _, _) => Err(HandleError::IncorrectResourceType),
            Resource::Image(image, vma_alloc, ici_owned, _) => {
                ma.resource = Resource::Image(*image, *vma_alloc, *ici_owned, new_layout);
                Ok(())
            }
        }
    }
}

impl Drop for ManagedAllocationHandle {
    fn drop(&mut self) {
        // If we are dropping this, this rc_ma and the one on the allocator still exists
        // That means that this last "user available" handle goes out of scope.
        // We must free the resource (if still not, checked by the allocator)
        if Rc::strong_count(&self.rc_ma) == 2 {
            let _ = unsafe { Self::free(&self) };
        } else {
        }
    }
}

enum BufferOrImage {
    Buffer(vk::Buffer),
    Image(vk::Image),
}

bitflags! {
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Extensions: u32 {
        const VK_KHR_get_memory_requirements2 = 0b00000001;
        const VK_KHR_dedicated_allocation = 0b00000010;
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
    api_version: u32,
    extensions: Extensions,
}

impl AllocatorSingleThread {
    pub fn new<'a>(
        instance: &'a ash::Instance,
        device: &'a ash::Device,
        physical_device: ash::vk::PhysicalDevice,
        queue: ash::vk::Queue,
        command_pool: ash::vk::CommandPool,
    ) -> AllocatorHandle {
        let api_version = unsafe {
            instance
                .get_physical_device_properties(physical_device)
                .api_version
        };
        let extensions = unsafe {
            let phy_extensions = instance
                .enumerate_device_extension_properties(physical_device)
                .unwrap();
            let has_extension = |looked: &CStr| {
                phy_extensions.iter().any(|extension| {
                    std::ffi::CStr::from_ptr(extension.extension_name.as_ptr()) == looked
                })
            };
            has_extension(&get_memory_requirements2::NAME)
                .then_some(Extensions::VK_KHR_get_memory_requirements2)
                .unwrap_or_default()
                | has_extension(&dedicated_allocation::NAME)
                    .then_some(Extensions::VK_KHR_dedicated_allocation)
                    .unwrap_or_default()
        };
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
            api_version,
            extensions,
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
                required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_CACHED,
                preferred_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
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

    fn get_hints(&self, resource: BufferOrImage) -> ResourceRequirementHints {
        if self.api_version >= vk::make_api_version(0, 1, 1, 0)
            || (self.extensions.contains(
                Extensions::VK_KHR_dedicated_allocation
                    | Extensions::VK_KHR_get_memory_requirements2,
            ))
        {
            let mut dedicated = vk::MemoryDedicatedRequirements::default();
            let mut mem_req2 = vk::MemoryRequirements2::default().push_next(&mut dedicated);
            match resource {
                BufferOrImage::Buffer(buffer) => {
                    let info_buffer = vk::BufferMemoryRequirementsInfo2::default().buffer(buffer);

                    unsafe {
                        self.device
                            .get_buffer_memory_requirements2(&info_buffer, &mut mem_req2);
                    }
                }
                BufferOrImage::Image(image) => {
                    let info_image = vk::ImageMemoryRequirementsInfo2::default().image(image);

                    unsafe {
                        self.device
                            .get_image_memory_requirements2(&info_image, &mut mem_req2);
                    }
                }
            }

            ResourceRequirementHints {
                size: mem_req2.memory_requirements.size,
                alignment: mem_req2.memory_requirements.alignment,
                memory_type_bits: mem_req2.memory_requirements.memory_type_bits,
                prefers_dedicated_allocation: dedicated.prefers_dedicated_allocation == vk::TRUE,
                requires_dedicated_allocation: dedicated.requires_dedicated_allocation == vk::TRUE,
            }
        } else {
            let mem_req = unsafe {
                match resource {
                    BufferOrImage::Buffer(buffer) => {
                        self.device.get_buffer_memory_requirements(buffer)
                    }
                    BufferOrImage::Image(image) => self.device.get_image_memory_requirements(image),
                }
            };
            ResourceRequirementHints {
                size: mem_req.size,
                alignment: mem_req.alignment,
                memory_type_bits: mem_req.memory_type_bits,
                prefers_dedicated_allocation: false,
                requires_dedicated_allocation: false,
            }
        }
    }

    pub fn allocate_buffer(
        &mut self,
        buffer_create_info: vk::BufferCreateInfo<'_>,
        config: AllocationConfig,
        allocator_handle: AllocatorHandle,
    ) -> VkResult<ManagedAllocationHandle> {
        let aci = Self::aci(&config.usage);
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

        let hints = self.get_hints(BufferOrImage::Buffer(buffer));
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
            _config: config,
            resource: Resource::Buffer(buffer, allocation.get_raw(), buffer_create_info_owned),
            hints,
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
        config: AllocationConfig,
        allocator_handle: AllocatorHandle,
    ) -> VkResult<ManagedAllocationHandle> {
        let aci = Self::aci(&config.usage);
        let image_create_info = image_create_info.usage(
            image_create_info
                .usage
                .bitor(vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST),
        );
        let image_create_info_owned = image_create_info.try_into().unwrap();

        let image = unsafe { self.device.create_image(&image_create_info, None).unwrap() };
        let hints = self.get_hints(BufferOrImage::Image(image));
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
            _config: config,
            resource: Resource::Image(
                image,
                allocation.get_raw(),
                image_create_info_owned,
                image_create_info.initial_layout,
            ),
            hints,
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
            Resource::Image(image, allocation, _, _) => {
                self.device.destroy_image(*image, None);
                self.allocator
                    .free_memory(&mut crate::Allocation::from_raw(*allocation));
            }
        }
        ma.freed = true;
        Ok(())
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
    pub unsafe fn defrag(&mut self) -> DefragmentationStats {
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
                    Resource::Image(image, alloc, icio, layout) => Resource::Image(
                        unsafe {
                            destroy_images.push(image);
                            let ici = Into::<ImageCreateInfo<'_>>::into(&icio);
                            let new_image = self.device.create_image(&ici, None).unwrap();
                            self.device
                                .bind_image_memory(
                                    new_image,
                                    destination_info.device_memory,
                                    destination_info.offset,
                                )
                                .unwrap();
                            if layout != vk::ImageLayout::UNDEFINED {
                                // TODO: If the layout is vk::ImageLayout::GENERAL, would be ok to just copy from, we would not need barriers (just barrier_to??).
                                let barrier_from = vk::ImageMemoryBarrier::default()
                                    .src_access_mask(
                                        vk::AccessFlags::HOST_WRITE
                                            | vk::AccessFlags::TRANSFER_WRITE
                                            | vk::AccessFlags::MEMORY_WRITE,
                                    )
                                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .old_layout(layout)
                                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                                    .image(image)
                                    .subresource_range(
                                        vk::ImageSubresourceRange::default()
                                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                                            .level_count(vk::REMAINING_MIP_LEVELS)
                                            .layer_count(vk::REMAINING_ARRAY_LAYERS),
                                    );
                                let barrier_to = vk::ImageMemoryBarrier::default()
                                    .src_access_mask(vk::AccessFlags::NONE)
                                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .old_layout(vk::ImageLayout::UNDEFINED)
                                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                                    .image(new_image)
                                    .subresource_range(
                                        vk::ImageSubresourceRange::default()
                                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                                            .level_count(vk::REMAINING_MIP_LEVELS)
                                            .layer_count(vk::REMAINING_ARRAY_LAYERS),
                                    );

                                self.device.cmd_pipeline_barrier(
                                    self.command_buffer,
                                    vk::PipelineStageFlags::HOST | vk::PipelineStageFlags::TRANSFER,
                                    vk::PipelineStageFlags::TRANSFER,
                                    vk::DependencyFlags::empty(),
                                    &[],
                                    &[],
                                    &[barrier_from, barrier_to],
                                );
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
                                let revert = vk::ImageMemoryBarrier::default()
                                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                                    .dst_access_mask(vk::AccessFlags::HOST_READ)
                                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                                    .new_layout(layout)
                                    .image(new_image)
                                    .subresource_range(
                                        vk::ImageSubresourceRange::default()
                                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                                            .level_count(vk::REMAINING_MIP_LEVELS)
                                            .layer_count(vk::REMAINING_ARRAY_LAYERS),
                                    );

                                self.device.cmd_pipeline_barrier(
                                    self.command_buffer,
                                    vk::PipelineStageFlags::TRANSFER,
                                    vk::PipelineStageFlags::HOST | vk::PipelineStageFlags::TRANSFER,
                                    vk::DependencyFlags::BY_REGION,
                                    &[],
                                    &[],
                                    &[revert],
                                );
                            }

                            new_image
                        },
                        alloc,
                        icio,
                        layout,
                    ),
                };
                ma.mem_offset = (destination_info.device_memory, destination_info.offset);
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
        ctx.end()
    }
}

impl Drop for AllocatorSingleThread {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_fence(self.fence, None);
        }
    }
}
