//! The only GPU work framecorder does: one small compute dispatch per recorded
//! frame. It reads the compositor's scanout buffer in place and writes NV12
//! into dmabufs that the hardware encoder reads directly. No CPU copies.
//!
//! Its queue priority is picked by the caller, see `Priority`.

use std::ffi::{c_char, CStr, CString};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

use anyhow::{bail, Context, Result};
use ash::vk;

use crate::kms::ScanoutBuffer;

#[path = "gpu_copy.rs"]
mod copy;
use crate::lut::Lut;

const SHADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/convert.spv"));
const MAX_SOURCES: usize = 8;

const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    out_size: [u32; 2],
    lut_size: [u32; 2],
    y_stride: u32,
    uv_offset: u32,
    lut_step: f32,
    flags: u32,
}

/// Layout of the NV12 buffers, dictated by the encoder.
#[derive(Clone, Copy, Debug)]
pub struct Nv12Layout {
    pub width: u32,
    pub height: u32,
    pub y_stride: u32,
    pub uv_offset: u32,
    pub size: u32,
}

struct Source {
    fb_id: u32,
    /// Identity of the buffer behind the framebuffer id, see `kms::buffer_id`.
    buffer: u64,
    last_used: u64,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    /// One descriptor set and pre-recorded command buffer per target.
    sets: Vec<vk::DescriptorSet>,
    cmds: Vec<vk::CommandBuffer>,
}

pub struct Target {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    pub fd: OwnedFd,
}

pub struct Gpu {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    ext_fd: ash::khr::external_memory_fd::Device,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    queue_family: u32,
    cmd_pool: vk::CommandPool,
    desc_pool: vk::DescriptorPool,
    set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    shader: vk::ShaderModule,
    sampler: vk::Sampler,
    fence: vk::Fence,
    /// Two timestamps around the dispatch, to report what the shader costs.
    queries: vk::QueryPool,
    timestamp_ns: f64,
    lut_buffer: vk::Buffer,
    lut_memory: vk::DeviceMemory,
    params: Params,
    layout: Nv12Layout,
    targets: Vec<Target>,
    sources: Vec<Source>,
    tick: u64,
    /// Where images handed to us by someone else get copied before converting.
    copy: Option<CopyTarget>,
    physical_device: vk::PhysicalDevice,
}

/// How the shader reads the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// One bilinear read per pixel.
    Bilinear,
    /// Catmull-Rom: crisper, five reads per pixel.
    Sharp,
    /// Four averaged reads, for when the output shrinks the source a lot.
    Supersample,
}

/// How our GPU work ranks against the game's. The scanout buffer gets reused
/// a frame later, so work that waits for the game to leave the GPU idle can
/// miss frames in heavy games.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Priority {
    /// Only runs when the game leaves the GPU idle.
    Low,
    /// Takes turns with the game.
    Medium,
    /// Cuts in ahead of the game, briefly.
    High,
}

impl Priority {
    fn vk(self) -> vk::QueueGlobalPriorityKHR {
        match self {
            Priority::Low => vk::QueueGlobalPriorityKHR::LOW,
            Priority::Medium => vk::QueueGlobalPriorityKHR::MEDIUM,
            Priority::High => vk::QueueGlobalPriorityKHR::HIGH,
        }
    }
}

/// Extensions to enable beyond our own: fixed ones for the instance, and a
/// function giving the device ones for the physical device we pick.
pub struct Extensions {
    pub instance: Vec<CString>,
    pub device: Box<dyn Fn(vk::PhysicalDevice) -> Vec<CString>>,
}

/// Raw handles for handing our device to other Vulkan users.
pub struct NativeHandles {
    pub instance: vk::Instance,
    pub device: vk::Device,
    pub physical_device: vk::PhysicalDevice,
    pub queue: vk::Queue,
    pub queue_family: u32,
}

struct CopyTarget {
    pool: vk::CommandPool,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    width: u32,
    height: u32,
}

/// Framebuffer id used for the copy source.
const COPY_SOURCE: u32 = u32::MAX;

impl Gpu {
    /// `extra` adds extensions someone else needs on our instance and device
    /// (SteamVR, to hand us its headset view).
    pub fn new(
        layout: Nv12Layout,
        target_count: usize,
        lut: &Lut,
        filter: Filter,
        priority: Priority,
        extra: Option<&Extensions>,
    ) -> Result<Self> {
        let entry = unsafe { ash::Entry::load() }.context("loading Vulkan")?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"framecorder")
            .api_version(vk::API_VERSION_1_1);
        let instance_exts: Vec<*const c_char> = extra.map_or_else(Vec::new, |e| e.instance.iter().map(|x| x.as_ptr()).collect());
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&instance_exts),
                None,
            )
        }
        .context("creating Vulkan instance")?;

        Self::with_instance(entry, instance, layout, target_count, lut, filter, priority, extra)
    }

    #[allow(clippy::too_many_arguments)]
    fn with_instance(
        entry: ash::Entry,
        instance: ash::Instance,
        layout: Nv12Layout,
        target_count: usize,
        lut: &Lut,
        filter: Filter,
        priority: Priority,
        extra: Option<&Extensions>,
    ) -> Result<Self> {
        let pdev = unsafe { instance.enumerate_physical_devices() }?
            .into_iter()
            .next()
            .context("no Vulkan device")?;
        let props = unsafe { instance.get_physical_device_properties(pdev) };
        let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
        log::info!("gpu: {}", name.to_string_lossy());

        let queue_family = unsafe { instance.get_physical_device_queue_family_properties(pdev) }
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE))
            .context("no compute queue")? as u32;

        let mut exts: Vec<*const c_char> = vec![
            ash::khr::external_memory_fd::NAME.as_ptr(),
            ash::ext::external_memory_dma_buf::NAME.as_ptr(),
            ash::ext::image_drm_format_modifier::NAME.as_ptr(),
            // Required by the modifier extension on Vulkan 1.1.
            ash::khr::image_format_list::NAME.as_ptr(),
            ash::ext::queue_family_foreign::NAME.as_ptr(),
        ];
        let wanted = extra.map(|e| (e.device)(pdev)).unwrap_or_default();
        for name in &wanted {
            if !exts.iter().any(|&e| unsafe { CStr::from_ptr(e) } == name.as_c_str()) {
                exts.push(name.as_ptr());
            }
        }
        // Last, so it can be dropped if the driver refuses the priority.
        exts.push(ash::ext::global_priority::NAME.as_ptr());
        let priorities = [0.0f32];
        let device = [true, false]
            .into_iter()
            .find_map(|with_priority| {
                let mut prio = vk::DeviceQueueGlobalPriorityCreateInfoKHR::default().global_priority(priority.vk());
                let mut qinfo = vk::DeviceQueueCreateInfo::default()
                    .queue_family_index(queue_family)
                    .queue_priorities(&priorities);
                if with_priority {
                    qinfo = qinfo.push_next(&mut prio);
                }
                let qinfos = [qinfo];
                let n = if with_priority { exts.len() } else { exts.len() - 1 };
                let info = vk::DeviceCreateInfo::default()
                    .queue_create_infos(&qinfos)
                    .enabled_extension_names(&exts[..n]);
                match unsafe { instance.create_device(pdev, &info, None) } {
                    Ok(d) => Some(d),
                    Err(e) => {
                        log::warn!("device creation failed ({priority:?} priority queue: {with_priority}): {e}");
                        None
                    }
                }
            })
            .context("creating Vulkan device")?;

        let ext_fd = ash::khr::external_memory_fd::Device::new(&instance, &device);
        let mem_props = unsafe { instance.get_physical_device_memory_properties(pdev) };
        let queue = unsafe { device.get_device_queue(queue_family, 0) };

        let mut gpu = Self {
            _entry: entry,
            instance,
            device,
            ext_fd,
            mem_props,
            timestamp_ns: props.limits.timestamp_period as f64,
            queue,
            queue_family,
            cmd_pool: vk::CommandPool::null(),
            desc_pool: vk::DescriptorPool::null(),
            set_layout: vk::DescriptorSetLayout::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            shader: vk::ShaderModule::null(),
            sampler: vk::Sampler::null(),
            fence: vk::Fence::null(),
            queries: vk::QueryPool::null(),
            lut_buffer: vk::Buffer::null(),
            lut_memory: vk::DeviceMemory::null(),
            params: Params {
                out_size: [layout.width, layout.height],
                lut_size: [lut.width, lut.height],
                y_stride: layout.y_stride,
                uv_offset: layout.uv_offset,
                lut_step: lut.step,
                flags: (lut.per_channel as u32)
                    | (((filter == Filter::Supersample) as u32) << 1)
                    | ((lut.rotated_eyes as u32) << 2)
                    | (((filter == Filter::Sharp) as u32) << 3),
            },
            layout,
            targets: Vec::new(),
            sources: Vec::new(),
            tick: 0,
            copy: None,
            physical_device: pdev,
        };
        gpu.create_pipeline(target_count)?;
        gpu.upload_lut(lut)?;
        for _ in 0..target_count {
            let t = gpu.create_target()?;
            gpu.targets.push(t);
        }
        Ok(gpu)
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    fn create_pipeline(&mut self, target_count: usize) -> Result<()> {
        let d = &self.device;
        unsafe {
            let code = ash::util::read_spv(&mut std::io::Cursor::new(SHADER))?;
            self.shader = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)?;

            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE),
            ];
            self.set_layout = d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?;

            let ranges = [vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .size(std::mem::size_of::<Params>() as u32)];
            let set_layouts = [self.set_layout];
            self.pipeline_layout = d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&ranges),
                None,
            )?;

            let stage = vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::COMPUTE)
                .module(self.shader)
                .name(c"main");
            let info = vk::ComputePipelineCreateInfo::default()
                .stage(stage)
                .layout(self.pipeline_layout);
            self.pipeline = d
                .create_compute_pipelines(vk::PipelineCache::null(), &[info], None)
                .map_err(|(_, e)| e)?[0];

            self.sampler = d.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )?;

            let max_sets = (MAX_SOURCES * target_count) as u32;
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(max_sets),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(max_sets * 2),
            ];
            self.desc_pool = d.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                    .max_sets(max_sets)
                    .pool_sizes(&sizes),
                None,
            )?;

            self.cmd_pool = d.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(self.queue_family),
                None,
            )?;
            self.fence = d.create_fence(&vk::FenceCreateInfo::default(), None)?;
            self.queries = d.create_query_pool(
                &vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2),
                None,
            )?;
        }
        Ok(())
    }

    fn find_memory_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
        (0..self.mem_props.memory_type_count).find(|&i| {
            bits & (1 << i) != 0
                && self.mem_props.memory_types[i as usize].property_flags.contains(want)
        })
    }

    fn upload_lut(&mut self, lut: &Lut) -> Result<()> {
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(lut.points.as_ptr().cast(), std::mem::size_of_val(lut.points.as_slice()))
        };
        let d = &self.device;
        unsafe {
            self.lut_buffer = d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(bytes.len() as u64)
                    .usage(vk::BufferUsageFlags::STORAGE_BUFFER),
                None,
            )?;
            let req = d.get_buffer_memory_requirements(self.lut_buffer);
            let ty = self
                .find_memory_type(
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
                .context("no host visible memory for the lookup grid")?;
            self.lut_memory = d.allocate_memory(
                &vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty),
                None,
            )?;
            d.bind_buffer_memory(self.lut_buffer, self.lut_memory, 0)?;
            let ptr = d.map_memory(self.lut_memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast(), bytes.len());
            d.unmap_memory(self.lut_memory);
        }
        Ok(())
    }

    fn create_target(&self) -> Result<Target> {
        let d = &self.device;
        let handle = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        unsafe {
            let mut ext = vk::ExternalMemoryBufferCreateInfo::default().handle_types(handle);
            let buffer = d.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(self.layout.size as u64)
                    .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
                    .push_next(&mut ext),
                None,
            )?;
            let req = d.get_buffer_memory_requirements(buffer);
            let ty = self
                .find_memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
                .context("no device memory for encoder buffers")?;
            let mut export = vk::ExportMemoryAllocateInfo::default().handle_types(handle);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().buffer(buffer);
            let memory = d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty)
                    .push_next(&mut export)
                    .push_next(&mut dedicated),
                None,
            )?;
            d.bind_buffer_memory(buffer, memory, 0)?;
            let fd = self.ext_fd.get_memory_fd(
                &vk::MemoryGetFdInfoKHR::default().memory(memory).handle_type(handle),
            )?;
            Ok(Target { buffer, memory, fd: OwnedFd::from_raw_fd(fd) })
        }
    }

    pub fn has_source(&self, fb_id: u32, buffer: u64) -> bool {
        self.sources.iter().any(|s| s.fb_id == fb_id && s.buffer == buffer)
    }

    /// Forgets every imported scanout buffer.
    pub fn clear_sources(&mut self) {
        let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.sources).into_iter().partition(|s| s.fb_id == COPY_SOURCE);
        self.sources = keep;
        for src in gone {
            self.destroy_source(src);
        }
    }

    /// Imports a scanout buffer. The compositor flips between a few buffers,
    /// so this happens a couple of times at startup and then never again.
    pub fn add_source(&mut self, buf: ScanoutBuffer, buffer: u64) -> Result<()> {
        // A recycled framebuffer id now points at a different buffer.
        if let Some(i) = self.sources.iter().position(|s| s.fb_id == buf.fb_id) {
            let src = self.sources.swap_remove(i);
            self.destroy_source(src);
        }
        if self.sources.len() >= MAX_SOURCES {
            let oldest = (0..self.sources.len())
                .min_by_key(|&i| self.sources[i].last_used)
                .unwrap();
            let src = self.sources.swap_remove(oldest);
            self.destroy_source(src);
        }
        let mut src = Source {
            fb_id: buf.fb_id,
            buffer,
            last_used: 0,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sets: Vec::new(),
            cmds: Vec::new(),
        };
        match self.import(buf, &mut src) {
            Ok(()) => {
                self.sources.push(src);
                Ok(())
            }
            Err(e) => {
                self.destroy_source(src);
                Err(e)
            }
        }
    }

    /// Fills in `src`; whatever got created is in there even on failure.
    fn import(&self, buf: ScanoutBuffer, src: &mut Source) -> Result<()> {
        let format = match buf.fourcc {
            DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888 => vk::Format::B8G8R8A8_UNORM,
            DRM_FORMAT_XBGR8888 | DRM_FORMAT_ABGR8888 => vk::Format::R8G8B8A8_UNORM,
            other => bail!("unsupported scanout format {:?}", other.to_le_bytes().map(char::from)),
        };
        log::info!(
            "importing scanout fb {} ({}x{}, modifier {:#x}, pitch {})",
            buf.fb_id,
            buf.width,
            buf.height,
            buf.modifier,
            buf.pitch
        );

        let d = &self.device;
        let handle = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        unsafe {
            let planes = [vk::SubresourceLayout {
                offset: buf.offset as u64,
                size: 0,
                row_pitch: buf.pitch as u64,
                array_pitch: 0,
                depth_pitch: 0,
            }];
            let mut modifier = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(buf.modifier)
                .plane_layouts(&planes);
            let mut ext = vk::ExternalMemoryImageCreateInfo::default().handle_types(handle);
            src.image = d
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D { width: buf.width, height: buf.height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                        .usage(vk::ImageUsageFlags::SAMPLED)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED)
                        .push_next(&mut modifier)
                        .push_next(&mut ext),
                    None,
                )
                .context("creating image for the scanout buffer (modifier not supported?)")?;
            let image = src.image;

            let mut fd_props = vk::MemoryFdPropertiesKHR::default();
            self.ext_fd
                .get_memory_fd_properties(handle, buf.fd.as_raw_fd(), &mut fd_props)?;
            let req = d.get_image_memory_requirements(image);
            let ty = self
                .find_memory_type(req.memory_type_bits & fd_props.memory_type_bits, vk::MemoryPropertyFlags::empty())
                .context("no memory type can import the scanout buffer")?;

            // Vulkan takes ownership of the fd on success.
            let fd = buf.fd.into_raw_fd();
            let mut import = vk::ImportMemoryFdInfoKHR::default().handle_type(handle).fd(fd);
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            src.memory = match d.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty)
                    .push_next(&mut import)
                    .push_next(&mut dedicated),
                None,
            ) {
                Ok(m) => m,
                Err(e) => {
                    libc::close(fd);
                    return Err(e).context("importing scanout memory");
                }
            };
            d.bind_image_memory(image, src.memory, 0)?;

            src.view = d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )?;

            let layouts = vec![self.set_layout; self.targets.len()];
            src.sets = d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.desc_pool)
                    .set_layouts(&layouts),
            )?;
            src.cmds = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.cmd_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(self.targets.len() as u32),
            )?;

            for (i, target) in self.targets.iter().enumerate() {
                self.write_set(src.sets[i], src.view, target);
                self.record(src.cmds[i], src.sets[i], image, target, true)?;
            }
            Ok(())
        }
    }

    unsafe fn write_set(&self, set: vk::DescriptorSet, view: vk::ImageView, target: &Target) {
        let image_info = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::GENERAL)];
        let lut_info = [vk::DescriptorBufferInfo::default().buffer(self.lut_buffer).range(vk::WHOLE_SIZE)];
        let out_info = [vk::DescriptorBufferInfo::default().buffer(target.buffer).range(vk::WHOLE_SIZE)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_info),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&lut_info),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&out_info),
        ];
        self.device.update_descriptor_sets(&writes, &[]);
    }

    /// `external` images belong to someone else (the display) and get
    /// borrowed and handed back around the dispatch; our own copy doesn't.
    unsafe fn record(
        &self,
        cmd: vk::CommandBuffer,
        set: vk::DescriptorSet,
        image: vk::Image,
        target: &Target,
        external: bool,
    ) -> Result<()> {
        let d = &self.device;
        let foreign = vk::QUEUE_FAMILY_FOREIGN_EXT;
        let ours = self.queue_family;
        let (img_from, img_to) = if external { (foreign, ours) } else { (vk::QUEUE_FAMILY_IGNORED, vk::QUEUE_FAMILY_IGNORED) };
        d.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default())?;
        d.cmd_reset_query_pool(cmd, self.queries, 0, 2);

        // Borrow the scanout image from the display and the NV12 buffer from
        // the encoder. GENERAL in and out, so nothing gets decompressed or
        // discarded behind the compositor's back.
        let acquire_image = vk::ImageMemoryBarrier::default()
            .src_queue_family_index(img_from)
            .dst_queue_family_index(img_to)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(image)
            .subresource_range(color_range());
        let acquire_buffer = vk::BufferMemoryBarrier::default()
            .src_queue_family_index(foreign)
            .dst_queue_family_index(ours)
            .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
            .buffer(target.buffer)
            .size(vk::WHOLE_SIZE);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[acquire_buffer],
            &[acquire_image],
        );

        d.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline);
        d.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline_layout, 0, &[set], &[]);
        let params: [u8; std::mem::size_of::<Params>()] = std::mem::transmute(self.params);
        d.cmd_push_constants(cmd, self.pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, &params);
        let groups_x = (self.layout.width / 4).div_ceil(8);
        let groups_y = (self.layout.height / 2).div_ceil(8);
        d.cmd_write_timestamp(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, self.queries, 0);
        d.cmd_dispatch(cmd, groups_x, groups_y, 1);
        d.cmd_write_timestamp(cmd, vk::PipelineStageFlags::BOTTOM_OF_PIPE, self.queries, 1);

        let release_image = vk::ImageMemoryBarrier::default()
            .src_queue_family_index(img_to)
            .dst_queue_family_index(img_from)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .image(image)
            .subresource_range(color_range());
        let release_buffer = vk::BufferMemoryBarrier::default()
            .src_queue_family_index(ours)
            .dst_queue_family_index(foreign)
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .buffer(target.buffer)
            .size(vk::WHOLE_SIZE);
        d.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::DependencyFlags::empty(),
            &[],
            &[release_buffer],
            &[release_image],
        );
        d.end_command_buffer(cmd)?;
        Ok(())
    }

    /// Converts the given scanout buffer into target `index` and waits for the
    /// GPU to finish, so the encoder can take the buffer right away.
    /// Returns how long the shader ran on the GPU.
    pub fn convert(&mut self, fb_id: u32, buffer: u64, index: usize) -> Result<std::time::Duration> {
        self.tick += 1;
        let tick = self.tick;
        let src = self
            .sources
            .iter_mut()
            .find(|s| s.fb_id == fb_id && s.buffer == buffer)
            .context("scanout buffer wasn't imported")?;
        src.last_used = tick;
        let cmd = src.cmds[index];
        self.submit(&[cmd])
    }

    /// Runs command buffers, waits for them, and returns the conversion
    /// shader's GPU time from the timestamps it wrote.
    fn submit(&self, cmds: &[vk::CommandBuffer]) -> Result<std::time::Duration> {
        unsafe {
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(cmds)],
                self.fence,
            )?;
            self.device.wait_for_fences(&[self.fence], true, 1_000_000_000)?;
            self.device.reset_fences(&[self.fence])?;

            let mut stamps = [0u64; 2];
            self.device
                .get_query_pool_results(self.queries, 0, &mut stamps, vk::QueryResultFlags::TYPE_64)?;
            let ns = stamps[1].saturating_sub(stamps[0]) as f64 * self.timestamp_ns;
            Ok(std::time::Duration::from_nanos(ns as u64))
        }
    }

    fn destroy_source(&self, src: Source) {
        unsafe {
            let d = &self.device;
            let _ = d.device_wait_idle();
            if !src.cmds.is_empty() {
                d.free_command_buffers(self.cmd_pool, &src.cmds);
            }
            if !src.sets.is_empty() {
                let _ = d.free_descriptor_sets(self.desc_pool, &src.sets);
            }
            d.destroy_image_view(src.view, None);
            d.destroy_image(src.image, None);
            d.free_memory(src.memory, None);
        }
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for src in std::mem::take(&mut self.sources) {
                self.destroy_source(src);
            }
            let d = &self.device;
            if let Some(copy) = self.copy.take() {
                d.destroy_command_pool(copy.pool, None);
            }
            for t in self.targets.drain(..) {
                d.destroy_buffer(t.buffer, None);
                d.free_memory(t.memory, None);
            }
            d.destroy_buffer(self.lut_buffer, None);
            d.free_memory(self.lut_memory, None);
            d.destroy_fence(self.fence, None);
            d.destroy_query_pool(self.queries, None);
            d.destroy_command_pool(self.cmd_pool, None);
            d.destroy_descriptor_pool(self.desc_pool, None);
            d.destroy_sampler(self.sampler, None);
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_pipeline_layout(self.pipeline_layout, None);
            d.destroy_descriptor_set_layout(self.set_layout, None);
            d.destroy_shader_module(self.shader, None);
            d.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
