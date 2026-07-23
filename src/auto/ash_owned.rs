use std::ptr::null;

use ash::vk::{
    BufferCreateFlags, BufferCreateInfo, BufferUsageFlags, DeviceSize, Extent3D, Format,
    ImageCreateFlags, ImageCreateInfo, ImageLayout, ImageTiling, ImageType, ImageUsageFlags,
    SampleCountFlags, SharingMode,
};

#[derive(Clone, Copy)]
pub struct BufferCreateInfoOwned {
    pub flags: BufferCreateFlags,
    pub size: DeviceSize,
    pub usage: BufferUsageFlags,
}

#[derive(Debug, PartialEq)]
pub struct InvalidBufferCreateInfo(&'static str);

impl TryFrom<BufferCreateInfo<'_>> for BufferCreateInfoOwned {
    type Error = InvalidBufferCreateInfo;

    fn try_from(from: BufferCreateInfo<'_>) -> Result<Self, Self::Error> {
        if from.p_next != null() {
            Err(InvalidBufferCreateInfo(
                "vk_mem_auto can't alloc a buffer with a p_next structure assigned",
            ))
        } else if from.sharing_mode != SharingMode::EXCLUSIVE {
            Err(InvalidBufferCreateInfo(
                "vk_mem_auto does not support VK_SHARING_MODE_CONCURRENT buffers",
            ))
        } else {
            Ok(Self {
                flags: from.flags,
                size: from.size,
                usage: from.usage,
            })
        }
    }
}

impl From<&BufferCreateInfoOwned> for BufferCreateInfo<'_> {
    fn from(from: &BufferCreateInfoOwned) -> Self {
        BufferCreateInfo::default()
            .flags(from.flags)
            .size(from.size)
            .usage(from.usage)
    }
}

#[derive(Clone, Copy)]
pub struct ImageCreateInfoOwned {
    pub flags: ImageCreateFlags,
    pub image_type: ImageType,
    pub format: Format,
    pub extent: Extent3D,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: SampleCountFlags,
    pub tiling: ImageTiling,
    pub usage: ImageUsageFlags,
    pub initial_layout: ImageLayout,
}

#[derive(Debug, PartialEq)]
pub struct InvalidImageCreateInfo(&'static str);

impl TryFrom<ImageCreateInfo<'_>> for ImageCreateInfoOwned {
    type Error = InvalidImageCreateInfo;

    fn try_from(from: ImageCreateInfo<'_>) -> Result<Self, Self::Error> {
        if from.p_next != null() {
            Err(InvalidImageCreateInfo(
                "vk_mem_auto can't alloc a image with a p_next structure assigned",
            ))
        } else if from.sharing_mode != SharingMode::EXCLUSIVE {
            Err(InvalidImageCreateInfo(
                "vk_mem_auto does not support VK_SHARING_MODE_CONCURRENT images",
            ))
        } else {
            Ok(Self {
                flags: from.flags,
                image_type: from.image_type,
                format: from.format,
                extent: from.extent,
                mip_levels: from.mip_levels,
                array_layers: from.array_layers,
                samples: from.samples,
                tiling: from.tiling,
                usage: from.usage,
                initial_layout: from.initial_layout,
            })
        }
    }
}

impl From<&ImageCreateInfoOwned> for ImageCreateInfo<'_> {
    fn from(from: &ImageCreateInfoOwned) -> Self {
        ImageCreateInfo::default()
            .flags(from.flags)
            .image_type(from.image_type)
            .format(from.format)
            .extent(from.extent)
            .mip_levels(from.mip_levels)
            .array_layers(from.array_layers)
            .samples(from.samples)
            .tiling(from.tiling)
            .usage(from.usage)
            .initial_layout(from.initial_layout)
    }
}
