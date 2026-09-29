//! Hands the tab's pixels to SteamVR as a Vulkan texture. SteamVR copies it
//! in one go, so the tab never flashes empty between updates the way it
//! does with SetOverlayRaw.

use std::ffi::{c_char, c_void, CStr, CString};

use anyhow::{Context, Result};
use ash::vk;

use crate::openvr::OpenVr;

const COMPOSITOR: &str = "IVRCompositor_029";
// Slots in VR_IVRCompositor_FnTable (openvr_capi.h, IVRCompositor_029).
const SLOT_INSTANCE_EXTENSIONS: usize = 41;
const SLOT_DEVICE_EXTENSIONS: usize = 42;

const TEXTURE_VULKAN: i32 = 2;
const COLOR_SPACE_AUTO: i32 = 0;

/// VRVulkanTextureData_t
#[repr(C)]
pub struct VulkanTextureData {
    image: u64,
    device: *mut c_void,
    physical_device: *mut c_void,
    instance: *mut c_void,
    queue: *mut c_void,
    queue_family: u32,
    width: u32,
    height: u32,
    format: u32,
    sample_count: u32,
}

/// Texture_t
#[repr(C)]
pub struct Texture {
    pub handle: *const c_void,
    pub kind: i32,
    pub color_space: i32,
}

pub struct OverlayTexture {
    _entry: ash::Entry,
    instance: ash::Instance,
    pdev: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    queue_family: u32,
    image: vk::Image,
    image_memory: vk::DeviceMemory,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_ptr: *mut u8,
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    width: u32,
    height: u32,
    data: VulkanTextureData,
}

/// SteamVR's space separated extension list, as owned C strings.
fn extension_list(raw: &[u8]) -> Vec<CString> {
    let text = CStr::from_bytes_until_nul(raw).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    text.split_whitespace().filter_map(|e| CString::new(e).ok()).collect()
}

impl OverlayTexture {
    pub fn new(vr: &OpenVr, width: u32, height: u32) -> Result<Self> {
        let compositor = vr.interface(COMPOSITOR).context("SteamVR doesn't offer IVRCompositor_029")?;
        let slot = |i: usize| unsafe { *compositor.add(i) };

        let mut buf = vec![0u8; 4096];
        let instance_exts = unsafe {
            let f: unsafe extern "C" fn(*mut c_char, u32) -> u32 = std::mem::transmute(slot(SLOT_INSTANCE_EXTENSIONS));
            f(buf.as_mut_ptr().cast(), buf.len() as u32);
            extension_list(&buf)
        };

        let entry = unsafe { ash::Entry::load() }.context("loading Vulkan")?;
        let app = vk::ApplicationInfo::default().application_name(c"framecorder-ui").api_version(vk::API_VERSION_1_1);
        let ext_ptrs: Vec<*const c_char> = instance_exts.iter().map(|e| e.as_ptr()).collect();
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&ext_ptrs),
                None,
            )
        }
        .context("creating a Vulkan instance for the tab")?;

        let pdev = unsafe { instance.enumerate_physical_devices() }?
            .into_iter()
            .next()
            .context("no Vulkan device")?;
        buf.fill(0);
        let device_exts = unsafe {
            let f: unsafe extern "C" fn(*mut c_void, *mut c_char, u32) -> u32 =
                std::mem::transmute(slot(SLOT_DEVICE_EXTENSIONS));
            f(std::mem::transmute::<vk::PhysicalDevice, *mut c_void>(pdev), buf.as_mut_ptr().cast(), buf.len() as u32);
            extension_list(&buf)
        };

        let queue_family = unsafe { instance.get_physical_device_queue_family_properties(pdev) }
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .context("no graphics queue")? as u32;
        let priorities = [0.0f32];
        let qinfo = [vk::DeviceQueueCreateInfo::default().queue_family_index(queue_family).queue_priorities(&priorities)];
        let dev_ptrs: Vec<*const c_char> = device_exts.iter().map(|e| e.as_ptr()).collect();
        let device = unsafe {
            instance.create_device(
                pdev,
                &vk::DeviceCreateInfo::default().queue_create_infos(&qinfo).enabled_extension_names(&dev_ptrs),
                None,
            )
        }
        .context("creating a Vulkan device for the tab")?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let mut tex = Self {
            _entry: entry,
            instance,
            pdev,
            device,
            queue,
            queue_family,
            image: vk::Image::null(),
            image_memory: vk::DeviceMemory::null(),
            staging: vk::Buffer::null(),
            staging_memory: vk::DeviceMemory::null(),
            staging_ptr: std::ptr::null_mut(),
            pool: vk::CommandPool::null(),
            cmd: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            width,
            height,
            data: unsafe { std::mem::zeroed() },
        };
        tex.create_resources()?;
        Ok(tex)
    }

    fn memory_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
        let props = unsafe { self.instance.get_physical_device_memory_properties(self.pdev) };
        (0..props.memory_type_count)
            .find(|&i| bits & (1 << i) != 0 && props.memory_types[i as usize].property_flags.contains(want))
    }

    fn create_resources(&mut self) -> Result<()> {
        let d = &self.device;
        unsafe {
            self.image = d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .extent(vk::Extent3D { width: self.width, height: self.height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::SAMPLED)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let req = d.get_image_memory_requirements(self.image);
            let ty = self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL).context("no memory for the tab image")?;
            self.image_memory = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None)?;
            d.bind_image_memory(self.image, self.image_memory, 0)?;

            let size = (self.width * self.height * 4) as u64;
            self.staging = d.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(vk::BufferUsageFlags::TRANSFER_SRC), None)?;
            let req = d.get_buffer_memory_requirements(self.staging);
            let ty = self
                .memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
                .context("no memory for the tab upload buffer")?;
            self.staging_memory = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None)?;
            d.bind_buffer_memory(self.staging, self.staging_memory, 0)?;
            self.staging_ptr = d.map_memory(self.staging_memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?.cast();

            self.pool = d.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(self.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            self.cmd = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default().command_pool(self.pool).command_buffer_count(1),
            )?[0];
            self.fence = d.create_fence(&vk::FenceCreateInfo::default(), None)?;

            use ash::vk::Handle;
            self.data = VulkanTextureData {
                image: self.image.as_raw(),
                device: self.device.handle().as_raw() as *mut c_void,
                physical_device: self.pdev.as_raw() as *mut c_void,
                instance: self.instance.handle().as_raw() as *mut c_void,
                queue: self.queue.as_raw() as *mut c_void,
                queue_family: self.queue_family,
                width: self.width,
                height: self.height,
                format: vk::Format::R8G8B8A8_UNORM.as_raw() as u32,
                sample_count: 1,
            };
        }
        Ok(())
    }

    /// Copies RGBA pixels into the image and leaves it ready for SteamVR to read.
    pub fn upload(&mut self, rgba: &[u8]) -> Result<Texture> {
        let d = &self.device;
        let size = (self.width * self.height * 4) as usize;
        anyhow::ensure!(rgba.len() == size, "pixel buffer is the wrong size");
        unsafe {
            // SteamVR copies with our queue, so make sure it's done with the last frame.
            d.queue_wait_idle(self.queue)?;
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), self.staging_ptr, size);

            d.reset_command_buffer(self.cmd, vk::CommandBufferResetFlags::empty())?;
            d.begin_command_buffer(self.cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
            let to_dst = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(self.image)
                .subresource_range(range);
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_dst],
            );
            let region = vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_extent(vk::Extent3D { width: self.width, height: self.height, depth: 1 });
            d.cmd_copy_buffer_to_image(self.cmd, self.staging, self.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
            // SteamVR expects textures it's handed in TRANSFER_SRC_OPTIMAL.
            let to_src = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                .image(self.image)
                .subresource_range(range);
            d.cmd_pipeline_barrier(
                self.cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_src],
            );
            d.end_command_buffer(self.cmd)?;
            let cmds = [self.cmd];
            d.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&cmds)], self.fence)?;
            d.wait_for_fences(&[self.fence], true, 1_000_000_000)?;
            d.reset_fences(&[self.fence])?;
        }
        Ok(Texture {
            handle: (&self.data as *const VulkanTextureData).cast(),
            kind: TEXTURE_VULKAN,
            color_space: COLOR_SPACE_AUTO,
        })
    }
}

impl Drop for OverlayTexture {
    fn drop(&mut self) {
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            d.destroy_fence(self.fence, None);
            d.destroy_command_pool(self.pool, None);
            d.destroy_buffer(self.staging, None);
            d.free_memory(self.staging_memory, None);
            d.destroy_image(self.image, None);
            d.free_memory(self.image_memory, None);
            d.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
