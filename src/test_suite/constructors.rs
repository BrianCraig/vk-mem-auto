use ash::vk;

pub(crate) fn ica_linear_1024_1024_rgba8<'a>() -> vk::ImageCreateInfo<'a> {
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
