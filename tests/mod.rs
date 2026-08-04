extern crate ash;
extern crate vk_mem;

use ash::{ext::debug_utils, vk};
use std::{os::raw::c_void, sync::Arc};
use vk_mem::{Alloc, Allocation, ManagedAllocationHandle};

fn extension_names() -> Vec<*const i8> {
    vec![debug_utils::NAME.as_ptr()]
}

unsafe extern "system" fn vulkan_debug_callback(
    _message_severity: ash::vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_types: ash::vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const ash::vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> ash::vk::Bool32 {
    let p_callback_data = &*p_callback_data;
    println!(
        "{:?}",
        ::std::ffi::CStr::from_ptr(p_callback_data.p_message)
    );
    ash::vk::FALSE
}

pub struct TestHarness {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub device: ash::Device,
    pub physical_device: ash::vk::PhysicalDevice,
    pub debug_callback: ash::vk::DebugUtilsMessengerEXT,
    pub debug_report_loader: debug_utils::Instance,
    pub has_validation_layer: bool,
    pub queue: ash::vk::Queue,
    pub command_pool: ash::vk::CommandPool,
    pub command_buffer: ash::vk::CommandBuffer,
    pub fence: ash::vk::Fence,
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.debug_report_loader
                .destroy_debug_utils_messenger(self.debug_callback, None);
            self.instance.destroy_instance(None);
        }
    }
}
impl TestHarness {
    pub fn new() -> Self {
        let entry = unsafe { ash::Entry::load().unwrap() };

        let instance_version = unsafe {
            entry
                .try_enumerate_instance_version()
                .unwrap_or(Some(ash::vk::API_VERSION_1_0))
                .unwrap()
        };

        let app_name = ::std::ffi::CString::new("vk-mem testing").unwrap();
        let app_info = ash::vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&app_name)
            .engine_version(0)
            .api_version(instance_version);

        let mut layer_names_raw: Vec<*const i8> = vec![];
        let extension_names_raw = extension_names();

        let validation_layer_raw = std::ffi::CString::new("VK_LAYER_KHRONOS_validation").unwrap();
        let has_validation_layer = unsafe {
            entry
                .enumerate_instance_layer_properties()
                .unwrap()
                .iter()
                .any(|layer| {
                    std::ffi::CStr::from_ptr(layer.layer_name.as_ptr())
                        == validation_layer_raw.as_c_str()
                })
        };
        if has_validation_layer {
            layer_names_raw.push(validation_layer_raw.as_ptr());
        }

        let create_info = ash::vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extension_names_raw)
            .enabled_layer_names(&layer_names_raw);

        let instance: ash::Instance = unsafe {
            entry
                .create_instance(&create_info, None)
                .expect("Instance creation error")
        };

        let debug_info = ash::vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                ash::vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | ash::vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));

        let debug_report_loader = debug_utils::Instance::new(&entry, &instance);
        let debug_callback = unsafe {
            debug_report_loader
                .create_debug_utils_messenger(&debug_info, None)
                .unwrap()
        };

        let physical_devices = unsafe {
            instance
                .enumerate_physical_devices()
                .expect("Physical device error")
        };

        let physical_device = unsafe {
            *physical_devices
                .iter()
                .filter(|physical_device| {
                    let version = instance
                        .get_physical_device_properties(**physical_device)
                        .api_version;
                    ash::vk::api_version_major(version) == 1
                        && ash::vk::api_version_minor(version) >= 3
                })
                .next()
                .expect("Couldn't find suitable device.")
        };

        let queue_family_index = unsafe {
            instance
                .get_physical_device_queue_family_properties(physical_device)
                .iter()
                .enumerate()
                .find_map(|(index, family)| {
                    if family.queue_flags.contains(ash::vk::QueueFlags::GRAPHICS) {
                        Some(index as u32)
                    } else {
                        None
                    }
                })
                .expect("No graphics queue family found")
        };

        let queue_info = [ash::vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])];

        let device_create_info =
            ash::vk::DeviceCreateInfo::default().queue_create_infos(&queue_info);

        let device: ash::Device = unsafe {
            instance
                .create_device(physical_device, &device_create_info, None)
                .unwrap()
        };

        let queue = unsafe { device.get_device_queue(0, 0) };

        let command_pool = unsafe {
            device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(queue_family_index)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .unwrap()
        };

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

        TestHarness {
            entry,
            instance,
            device,
            physical_device,
            debug_report_loader,
            debug_callback,
            has_validation_layer,
            queue,
            command_buffer,
            command_pool,
            fence,
        }
    }

    pub fn create_allocator(&self) -> vk_mem::Allocator {
        let create_info =
            vk_mem::AllocatorCreateInfo::new(&self.instance, &self.device, self.physical_device);
        unsafe { vk_mem::Allocator::new(create_info).unwrap() }
    }

    pub fn create_allocator_single_thread(&self) -> vk_mem::AllocatorHandle {
        vk_mem::AllocatorSingleThread::new(
            &self.instance,
            &self.device,
            self.physical_device,
            self.queue,
            self.command_pool,
        )
    }
}

#[test]
fn create_harness() {
    let _ = TestHarness::new();
}

#[test]
fn create_allocator() {
    let harness = TestHarness::new();
    let _ = harness.create_allocator();
}

#[test]
fn create_gpu_buffer() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator();
    let allocation_info = vk_mem::AllocationCreateInfo {
        usage: vk_mem::MemoryUsage::Auto,
        ..Default::default()
    };

    unsafe {
        let (buffer, mut allocation) = allocator
            .create_buffer(
                &ash::vk::BufferCreateInfo::default().size(16 * 1024).usage(
                    ash::vk::BufferUsageFlags::VERTEX_BUFFER
                        | ash::vk::BufferUsageFlags::TRANSFER_DST,
                ),
                &allocation_info,
            )
            .unwrap();
        let allocation_info = allocator.get_allocation_info(&allocation);
        assert_eq!(allocation_info.mapped_data, std::ptr::null_mut());
        allocator.destroy_buffer(buffer, &mut allocation);
    }
}

#[test]
fn create_cpu_buffer_preferred() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator();
    let allocation_info = vk_mem::AllocationCreateInfo {
        required_flags: ash::vk::MemoryPropertyFlags::HOST_VISIBLE,
        preferred_flags: ash::vk::MemoryPropertyFlags::HOST_COHERENT
            | ash::vk::MemoryPropertyFlags::HOST_CACHED,
        flags: vk_mem::AllocationCreateFlags::MAPPED,
        ..Default::default()
    };
    unsafe {
        let (buffer, mut allocation) = allocator
            .create_buffer(
                &ash::vk::BufferCreateInfo::default().size(16 * 1024).usage(
                    ash::vk::BufferUsageFlags::VERTEX_BUFFER
                        | ash::vk::BufferUsageFlags::TRANSFER_DST,
                ),
                &allocation_info,
            )
            .unwrap();
        let allocation_info = allocator.get_allocation_info(&allocation);
        assert_ne!(allocation_info.mapped_data, std::ptr::null_mut());
        allocator.destroy_buffer(buffer, &mut allocation);
    }
}

#[test]
fn create_gpu_buffer_pool() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator();
    let allocator = Arc::new(allocator);

    let buffer_info = ash::vk::BufferCreateInfo::default()
        .size(16 * 1024)
        .usage(ash::vk::BufferUsageFlags::UNIFORM_BUFFER | ash::vk::BufferUsageFlags::TRANSFER_DST);

    let allocation_info = vk_mem::AllocationCreateInfo {
        required_flags: ash::vk::MemoryPropertyFlags::HOST_VISIBLE,
        preferred_flags: ash::vk::MemoryPropertyFlags::HOST_COHERENT
            | ash::vk::MemoryPropertyFlags::HOST_CACHED,
        flags: vk_mem::AllocationCreateFlags::MAPPED,

        ..Default::default()
    };
    unsafe {
        let memory_type_index = allocator
            .find_memory_type_index_for_buffer_info(&buffer_info, &allocation_info)
            .unwrap();

        // Create a pool that can have at most 2 blocks, 128 MiB each.
        let pool_info = vk_mem::PoolCreateInfo {
            memory_type_index,
            block_size: 128 * 1024 * 1024,
            max_block_count: 2,
            ..Default::default()
        };

        let pool = allocator.create_pool(&pool_info).unwrap();

        let (buffer, mut allocation) = pool.create_buffer(&buffer_info, &allocation_info).unwrap();
        let allocation_info = allocator.get_allocation_info(&allocation);
        assert_ne!(allocation_info.mapped_data, std::ptr::null_mut());
        allocator.destroy_buffer(buffer, &mut allocation);
    }
}

#[test]
fn test_gpu_stats() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator();
    let allocation_info = vk_mem::AllocationCreateInfo {
        usage: vk_mem::MemoryUsage::Auto,
        ..Default::default()
    };

    unsafe {
        let stats_1 = allocator.calculate_statistics().unwrap();
        assert_eq!(stats_1.total.statistics.blockCount, 0);
        assert_eq!(stats_1.total.statistics.allocationCount, 0);
        assert_eq!(stats_1.total.statistics.allocationBytes, 0);

        let (buffer, mut allocation) = allocator
            .create_buffer(
                &ash::vk::BufferCreateInfo::default().size(16 * 1024).usage(
                    ash::vk::BufferUsageFlags::VERTEX_BUFFER
                        | ash::vk::BufferUsageFlags::TRANSFER_DST,
                ),
                &allocation_info,
            )
            .unwrap();

        let stats_2 = allocator.calculate_statistics().unwrap();
        assert_eq!(stats_2.total.statistics.blockCount, 1);
        assert_eq!(stats_2.total.statistics.allocationCount, 1);
        assert_eq!(stats_2.total.statistics.allocationBytes, 16 * 1024);

        allocator.destroy_buffer(buffer, &mut allocation);

        let stats_3 = allocator.calculate_statistics().unwrap();
        assert_eq!(stats_3.total.statistics.blockCount, 1);
        assert_eq!(stats_3.total.statistics.allocationCount, 0);
        assert_eq!(stats_3.total.statistics.allocationBytes, 0);
    }
}

#[test]
fn create_virtual_block() {
    let create_info = vk_mem::VirtualBlockCreateInfo {
        size: 16 * 1024 * 1024,
        flags: vk_mem::VirtualBlockCreateFlags::VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT,
        allocation_callbacks: None,
    }; // 16MB block
    let _virtual_block =
        vk_mem::VirtualBlock::new(create_info).expect("Couldn't create VirtualBlock");
}

#[test]
fn virtual_allocate_and_free() {
    let create_info = vk_mem::VirtualBlockCreateInfo {
        size: 16 * 1024 * 1024,
        flags: vk_mem::VirtualBlockCreateFlags::VMA_VIRTUAL_BLOCK_CREATE_LINEAR_ALGORITHM_BIT,
        allocation_callbacks: None,
    }; // 16MB block
    let mut virtual_block =
        vk_mem::VirtualBlock::new(create_info).expect("Couldn't create VirtualBlock");

    let allocation_info = vk_mem::VirtualAllocationCreateInfo {
        size: 8 * 1024 * 1024,
        alignment: 0,
        user_data: 0,
        flags: vk_mem::VirtualAllocationCreateFlags::empty(),
    };

    // Fully allocate the VirtualBlock and then free both allocations
    unsafe {
        let (mut virtual_alloc_0, offset_0) = virtual_block.allocate(allocation_info).unwrap();
        let (mut virtual_alloc_1, offset_1) = virtual_block.allocate(allocation_info).unwrap();
        assert_ne!(offset_0, offset_1);
        virtual_block.free(&mut virtual_alloc_0);
        virtual_block.free(&mut virtual_alloc_1);
    }

    // Fully allocate it again and then clear it
    unsafe {
        let (_virtual_alloc_0, offset_0) = virtual_block.allocate(allocation_info).unwrap();
        let (_virtual_alloc_1, offset_1) = virtual_block.allocate(allocation_info).unwrap();
        assert_ne!(offset_0, offset_1);
        virtual_block.clear();
    }

    // VMA should trigger an assert when the VirtualBlock is dropped, if any
    // allocations have not been freed, or the block not cleared instead
}

#[test]
fn virtual_allocation_user_data() {
    let create_info = vk_mem::VirtualBlockCreateInfo {
        size: 16 * 1024 * 1024,
        ..Default::default()
    }; // 16MB block
    let mut virtual_block =
        vk_mem::VirtualBlock::new(create_info).expect("Couldn't create VirtualBlock");

    let user_data = Box::new(vec![12, 34, 56, 78, 90]);
    let allocation_info = vk_mem::VirtualAllocationCreateInfo {
        size: 8 * 1024 * 1024,
        alignment: 0,
        user_data: user_data.as_ptr() as usize,
        flags: vk_mem::VirtualAllocationCreateFlags::empty(),
    };

    unsafe {
        let (mut virtual_alloc_0, _) = virtual_block.allocate(allocation_info).unwrap();
        let queried_info = virtual_block
            .get_allocation_info(&virtual_alloc_0)
            .expect("Couldn't get VirtualAllocationInfo from VirtualBlock");
        let queried_user_data = std::slice::from_raw_parts(queried_info.user_data as *const i32, 5);
        assert_eq!(queried_user_data, &*user_data);
        virtual_block.free(&mut virtual_alloc_0);
    }
}

#[test]
fn virtual_block_out_of_space() {
    let create_info = vk_mem::VirtualBlockCreateInfo {
        size: 16 * 1024 * 1024,
        ..Default::default()
    }; // 16MB block
    let mut virtual_block =
        vk_mem::VirtualBlock::new(create_info).expect("Couldn't create VirtualBlock");

    let allocation_info = vk_mem::VirtualAllocationCreateInfo {
        size: 16 * 1024 * 1024 + 1,
        alignment: 0,
        user_data: 0,
        flags: vk_mem::VirtualAllocationCreateFlags::empty(),
    };

    unsafe {
        match virtual_block.allocate(allocation_info) {
            Ok(_) => panic!("Created VirtualAllocation larger than VirtualBlock"),
            Err(ash::vk::Result::ERROR_OUT_OF_DEVICE_MEMORY) => {}
            Err(_) => panic!("Unexpected VirtualBlock error"),
        }
    }
}

#[test]
fn test_render_white_pixels() {
    let harness = TestHarness::new();

    let api_version = unsafe {
        ash::Entry::load()
            .unwrap()
            .try_enumerate_instance_version()
            .unwrap_or(Some(ash::vk::API_VERSION_1_0))
            .unwrap()
    };

    let allocator_create_info = vk_mem::AllocatorCreateInfo::new(
        &harness.instance,
        &harness.device,
        harness.physical_device,
    )
    .vulkan_api_version(api_version);

    let allocator = unsafe { vk_mem::Allocator::new(allocator_create_info).unwrap() };

    let buffer_info = vk::BufferCreateInfo::default()
        .size(32 * 32 * 4)
        .usage(vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let alloc_info = vk_mem::AllocationCreateInfo {
        required_flags: vk::MemoryPropertyFlags::HOST_VISIBLE
            | vk::MemoryPropertyFlags::HOST_COHERENT,
        flags: vk_mem::AllocationCreateFlags::MAPPED
            | vk_mem::AllocationCreateFlags::HOST_ACCESS_RANDOM,
        ..Default::default()
    };

    let (buffer, mut buffer_alloc) =
        unsafe { allocator.create_buffer(&buffer_info, &alloc_info).unwrap() };

    let image_info = ash::vk::ImageCreateInfo::default()
        .samples(vk::SampleCountFlags::TYPE_1)
        .mip_levels(1)
        .image_type(vk::ImageType::TYPE_2D)
        .array_layers(1)
        .extent(vk::Extent3D {
            height: 32,
            width: 32,
            depth: 1,
        })
        .format(vk::Format::B8G8R8A8_UNORM)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC);

    let image = unsafe { harness.device.create_image(&image_info, None).unwrap() };

    let mem_info = vk_mem::AllocationCreateInfo {
        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL
            | vk::MemoryPropertyFlags::HOST_VISIBLE,
        flags: vk_mem::AllocationCreateFlags::MAPPED
            | vk_mem::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE,
        ..Default::default()
    };

    let mut allocation = unsafe {
        allocator
            .allocate_memory_for_image(image, &mem_info)
            .unwrap()
    };

    unsafe {
        allocator.bind_image_memory(&allocation, image).unwrap();
    }

    let allocation_info = allocator.get_allocation_info(&allocation);
    assert_ne!(allocation_info.mapped_data, std::ptr::null_mut());

    let image_view = unsafe {
        harness
            .device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::B8G8R8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }),
                None,
            )
            .unwrap()
    };

    let color_attachment = vk::AttachmentDescription::default()
        .format(vk::Format::B8G8R8A8_UNORM)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL);

    let color_attachment_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(std::slice::from_ref(&color_attachment_ref));

    let dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        );

    let render_pass = unsafe {
        harness
            .device
            .create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(std::slice::from_ref(&color_attachment))
                    .subpasses(std::slice::from_ref(&subpass))
                    .dependencies(std::slice::from_ref(&dependency)),
                None,
            )
            .unwrap()
    };

    let framebuffer = unsafe {
        harness
            .device
            .create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(render_pass)
                    .attachments(std::slice::from_ref(&image_view))
                    .width(32)
                    .height(32)
                    .layers(1),
                None,
            )
            .unwrap()
    };

    let pipeline_layout = unsafe {
        harness
            .device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
            .unwrap()
    };
    let fragment_spv = include_bytes!("../shaders/fragment_white.spv");

    let fragment_code = ash::util::read_spv(&mut std::io::Cursor::new(fragment_spv)).unwrap();

    let fragment_shader = unsafe {
        harness
            .device
            .create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&fragment_code),
                None,
            )
            .unwrap()
    };

    let vertex_spv = include_bytes!("../shaders/vertex_fullscreen.spv");

    let vertex_code = ash::util::read_spv(&mut std::io::Cursor::new(vertex_spv)).unwrap();

    let vertex_shader = unsafe {
        harness
            .device
            .create_shader_module(
                &vk::ShaderModuleCreateInfo::default().code(&vertex_code),
                None,
            )
            .unwrap()
    };

    let shader_entry_vs = std::ffi::CString::new("main_vs").unwrap();
    let shader_entry_fs = std::ffi::CString::new("main_fs").unwrap();

    let shader_stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex_shader)
            .name(&shader_entry_vs),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment_shader)
            .name(&shader_entry_fs),
    ];

    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();

    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);

    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);

    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);

    let color_attachment = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA);

    let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(std::slice::from_ref(&color_attachment));

    let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];

    let dynamic_state = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);

    let pipeline = unsafe {
        harness
            .device
            .create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[vk::GraphicsPipelineCreateInfo::default()
                    .stages(&shader_stages)
                    .vertex_input_state(&vertex_input)
                    .input_assembly_state(&input_assembly)
                    .viewport_state(&viewport_state)
                    .rasterization_state(&rasterization)
                    .multisample_state(&multisample)
                    .color_blend_state(&color_blend)
                    .dynamic_state(&dynamic_state)
                    .layout(pipeline_layout)
                    .render_pass(render_pass)
                    .subpass(0)],
                None,
            )
            .unwrap()
            .remove(0)
    };

    unsafe {
        harness
            .device
            .begin_command_buffer(
                harness.command_buffer,
                &vk::CommandBufferBeginInfo::default(),
            )
            .unwrap();

        let clear = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0],
            },
        }];

        harness.device.cmd_begin_render_pass(
            harness.command_buffer,
            &vk::RenderPassBeginInfo::default()
                .render_pass(render_pass)
                .framebuffer(framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: 32,
                        height: 32,
                    },
                })
                .clear_values(&clear),
            vk::SubpassContents::INLINE,
        );

        harness.device.cmd_bind_pipeline(
            harness.command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            pipeline,
        );

        harness.device.cmd_set_viewport(
            harness.command_buffer,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: 32.0,
                height: 32.0,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );

        harness.device.cmd_set_scissor(
            harness.command_buffer,
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: 32,
                    height: 32,
                },
            }],
        );

        harness.device.cmd_draw(harness.command_buffer, 6, 2, 0, 0);

        harness.device.cmd_end_render_pass(harness.command_buffer);

        let region = vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .mip_level(0)
                    .base_array_layer(0)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: 32,
                height: 32,
                depth: 1,
            });

        harness.device.cmd_copy_image_to_buffer(
            harness.command_buffer,
            image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buffer,
            std::slice::from_ref(&region),
        );

        harness
            .device
            .end_command_buffer(harness.command_buffer)
            .unwrap();
    }

    unsafe {
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
            .wait_for_fences(&[harness.fence], true, u64::MAX)
            .unwrap();
    }

    let pixels = unsafe {
        std::slice::from_raw_parts(
            allocator.get_allocation_info(&buffer_alloc).mapped_data as *const u8,
            32 * 32 * 4,
        )
    };

    for pixel in pixels.chunks_exact(4) {
        assert_eq!(pixel, &[255, 255, 255, 255]);
    }

    unsafe {
        allocator.destroy_image(image, &mut allocation);
        allocator.destroy_buffer(buffer, &mut buffer_alloc);
    }

    unsafe {
        harness.device.destroy_pipeline(pipeline, None);
        harness.device.destroy_shader_module(vertex_shader, None);
        harness.device.destroy_shader_module(fragment_shader, None);
        harness
            .device
            .destroy_pipeline_layout(pipeline_layout, None);
        harness.device.destroy_framebuffer(framebuffer, None);
        harness.device.destroy_render_pass(render_pass, None);
        harness.device.destroy_image_view(image_view, None);
    }
}

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
        unsafe { allocator.destroy_buffer(buffer, &mut Allocation::from_raw(allocation_handle)) };
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

#[test]
fn auto_test() {
    let harness = TestHarness::new();
    let mut allocator = harness.create_allocator_single_thread();

    let mut handles = vec![];

    fn map_and_write(handle: &ManagedAllocationHandle, value: u8) {
        let size = handle.size().unwrap() as usize;
        handle
            .map(|pointer, flush| {
                unsafe {
                    pointer.write_bytes(value, size);
                }
                flush();
            })
            .unwrap();
    }

    for _ in 0..64 {
        handles.push(
            allocator
                .allocate_buffer(
                    vk::BufferCreateInfo::default()
                        .size(1024 * 256)
                        .usage(vk::BufferUsageFlags::VERTEX_BUFFER),
                    vk_mem::AllocationUsage::Readback,
                )
                .unwrap(),
        );
    }

    // Lets drop half of the allocations to make some space.
    // Dropping their handles (this are the only ones since we didn't `copy()` them)
    // makes them free the resource
    let mut index = 0;
    handles.retain(|_| {
        index += 1;
        index % 2 == 0
    });

    for _ in 0..8 {
        handles.push(
            allocator
                .allocate_buffer(
                    vk::BufferCreateInfo::default()
                        .size(1024 * 1024)
                        .usage(vk::BufferUsageFlags::VERTEX_BUFFER),
                    vk_mem::AllocationUsage::Readback,
                )
                .unwrap(),
        );
    }

    handles
        .iter()
        .enumerate()
        .for_each(|(index, handle)| map_and_write(handle, index as u8));

    println!("defrag pass stats: {:?}", unsafe { allocator.defrag() });

    for (index, handle) in handles.iter().enumerate() {
        let size = handle.size().unwrap() as usize;
        handle
            .map(|pointer, _| {
                let expected = u128::from_ne_bytes([index as u8; 16]);
                let slice = unsafe { std::slice::from_raw_parts(pointer, size) };
                for chunk in slice.chunks_exact(16) {
                    let actual = u128::from_ne_bytes(chunk.try_into().unwrap());
                    assert_eq!(actual, expected);
                }
            })
            .unwrap();
        unsafe { handle.free().unwrap() };
    }
}

#[test]
fn handle_incorrect_handlers_usage() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator_single_thread();

    let handle: ManagedAllocationHandle = allocator
        .allocate_buffer(
            vk::BufferCreateInfo::default()
                .size(1024 * 1024)
                .usage(vk::BufferUsageFlags::VERTEX_BUFFER),
            vk_mem::AllocationUsage::Readback,
        )
        .unwrap();

    assert!(handle.size().unwrap() == 1024 * 1024);

    assert!(unsafe { handle.free() }.is_ok());

    assert_eq!(handle.size(), Err(vk_mem::HandleError::FreedResource));
}

#[test]
fn upload_move_image() {
    let harness = TestHarness::new();
    let mut allocator = harness.create_allocator_single_thread();

    // We use _first_image since using _ would mean it is dropped instantly, this way its dropped at the end of the test
    let _first_image: ManagedAllocationHandle = allocator
        .allocate_image(ica_linear_1024_1024_rgba8(), vk_mem::AllocationUsage::Cpu)
        .unwrap();

    let second_image: ManagedAllocationHandle = allocator
        .allocate_image(ica_linear_1024_1024_rgba8(), vk_mem::AllocationUsage::Cpu)
        .unwrap();

    let third_image = allocator
        .allocate_image(ica_linear_1024_1024_rgba8(), vk_mem::AllocationUsage::Cpu)
        .unwrap();

    fill_image(&third_image, 0xfafafaff);

    assert_image(&third_image, 0xfafafaff);

    transition_image(&harness, &third_image, vk::ImageLayout::GENERAL);

    assert_image(&third_image, 0xfafafaff);

    drop(second_image);

    println!("defrag pass stats: {:?}", unsafe { allocator.defrag() });

    assert_image(&third_image, 0xfafafaff);
}

fn transition_image(
    harness: &TestHarness,
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

fn fill_image(handle: &ManagedAllocationHandle, value: u32) {
    let size = handle.size().unwrap() as usize;
    let _ = handle.map(|pointer, flush| unsafe {
        std::slice::from_raw_parts_mut(pointer.cast::<u32>(), size / 4).fill(value);
        flush();
    });
}

fn assert_image(handle: &ManagedAllocationHandle, value: u32) {
    let size = handle.size().unwrap() as usize;
    let count = size / 4;
    let _ = handle.map(|pointer, _| unsafe {
        assert!(std::slice::from_raw_parts_mut(pointer.cast::<u32>(), count)
            .iter()
            .copied()
            .all(|e| e == value));
    });
}

fn ica_linear_1024_1024_rgba8<'a>() -> vk::ImageCreateInfo<'a> {
    vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .array_layers(1)
        .mip_levels(1)
        .extent(vk::Extent3D {
            width: 1024,
            height: 1024,
            depth: 1,
        })
        .format(vk::Format::R8G8B8A8_SRGB)
        .initial_layout(vk::ImageLayout::PREINITIALIZED)
        .tiling(vk::ImageTiling::LINEAR)
        .samples(vk::SampleCountFlags::TYPE_1)
}

#[test]
fn hints_test() {
    let harness = TestHarness::new();
    let allocator = harness.create_allocator_single_thread();

    let buffer = allocator
        .allocate_buffer(
            vk::BufferCreateInfo::default()
                .size(1024 * 1024 * 256)
                .usage(vk::BufferUsageFlags::VERTEX_BUFFER),
            vk_mem::AllocationUsage::Cpu,
        )
        .unwrap();

    let image = allocator
        .allocate_image(ica_linear_1024_1024_rgba8(), vk_mem::AllocationUsage::Cpu)
        .unwrap();

    println!("buffer hints: {:?}", buffer.resource_hints());
    println!("image hints: {:?}", image.resource_hints());
}
