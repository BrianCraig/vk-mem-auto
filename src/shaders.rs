#[cfg(test)]
mod test {
    use ash::vk;

    use crate as vk_mem;
    use crate::test_suite::run::TestHarness;
    use crate::Alloc;
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
}
