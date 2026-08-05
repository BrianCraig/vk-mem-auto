use crate::{Allocator, AllocatorCreateInfo, AllocatorHandle, AllocatorSingleThread};

use ash::{ext::debug_utils, vk};
use std::os::raw::c_void;

fn extension_names() -> Vec<*const i8> {
    vec![debug_utils::NAME.as_ptr()]
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: ash::vk::DebugUtilsMessageSeverityFlagsEXT,
    message_types: ash::vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const ash::vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> ash::vk::Bool32 {
    let severity: &'static str = match message_severity {
        ash::vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => "Error",
        ash::vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => "Warning",
        ash::vk::DebugUtilsMessageSeverityFlagsEXT::INFO => "Info",
        ash::vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => "Verbose",
        _ => "Unknown",
    };
    let message_type = match message_types {
        ash::vk::DebugUtilsMessageTypeFlagsEXT::GENERAL => "General",
        ash::vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE => "Performance",
        ash::vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION => "Validation",
        ash::vk::DebugUtilsMessageTypeFlagsEXT::DEVICE_ADDRESS_BINDING => "DeviceAddressBinding",
        _ => "Unknown",
    };
    let p_callback_data = &*p_callback_data;
    println!(
        "{severity}[{message_type}] {:?}",
        ::std::ffi::CStr::from_ptr(p_callback_data.p_message)
    );
    ash::vk::FALSE
}

pub struct TestHarness {
    pub _entry: ash::Entry,
    pub instance: ash::Instance,
    pub device: ash::Device,
    pub physical_device: ash::vk::PhysicalDevice,
    pub debug_callback: ash::vk::DebugUtilsMessengerEXT,
    pub debug_report_loader: debug_utils::Instance,
    pub _has_validation_layer: bool,
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
            _entry: entry,
            instance,
            device,
            physical_device,
            debug_report_loader,
            debug_callback,
            _has_validation_layer: has_validation_layer,
            queue,
            command_buffer,
            command_pool,
            fence,
        }
    }

    pub fn create_allocator(&self) -> Allocator {
        let create_info =
            AllocatorCreateInfo::new(&self.instance, &self.device, self.physical_device);
        unsafe { Allocator::new(create_info).unwrap() }
    }

    pub fn create_allocator_single_thread(&self) -> AllocatorHandle {
        AllocatorSingleThread::new(
            &self.instance,
            &self.device,
            self.physical_device,
            self.queue,
            self.command_pool,
        )
    }
}
