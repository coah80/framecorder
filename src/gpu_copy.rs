//! Converting images someone else hands us on our own device (SteamVR's
//! headset view): each frame gets copied into an image of ours, then runs
//! through the same conversion as the panel scanout.

use anyhow::{Context, Result};
use ash::vk;

use super::{color_range, CopyTarget, Gpu, NativeHandles, Source, COPY_SOURCE};

impl Gpu {
    pub fn native(&self) -> NativeHandles {
        NativeHandles {
            instance: self.instance.handle(),
            device: self.device.handle(),
            physical_device: self.physical_device,
            queue: self.queue,
            queue_family: self.queue_family,
        }
    }

    /// Size of the image frames get copied into, if it's set up.
    pub fn copy_size(&self) -> Option<(u32, u32)> {
        self.copy.as_ref().map(|c| (c.width, c.height))
    }

    /// Sets up (or replaces) the image frames get copied into, `width`x`height` RGBA.
    pub fn add_copy_source(&mut self, width: u32, height: u32) -> Result<()> {
        if let Some(i) = self.sources.iter().position(|s| s.fb_id == COPY_SOURCE) {
            let old = self.sources.swap_remove(i);
            self.destroy_source(old);
        }
        if let Some(old) = self.copy.take() {
            unsafe { self.device.destroy_command_pool(old.pool, None) };
        }
        log::info!("copying {width}x{height} frames from SteamVR");
        let d = &self.device;
        let mut src = Source {
            fb_id: COPY_SOURCE,
            buffer: 0,
            last_used: 0,
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            sets: Vec::new(),
            cmds: Vec::new(),
        };
        let format = vk::Format::R8G8B8A8_UNORM;
        unsafe {
            src.image = d.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D { width, height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let req = d.get_image_memory_requirements(src.image);
            let ty = self
                .find_memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)
                .context("no memory for the copy image")?;
            src.memory = d.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None)?;
            d.bind_image_memory(src.image, src.memory, 0)?;
            src.view = d.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(src.image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )?;
            let layouts = vec![self.set_layout; self.targets.len()];
            src.sets = d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.desc_pool).set_layouts(&layouts),
            )?;
            src.cmds = d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.cmd_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(self.targets.len() as u32),
            )?;
            for (i, target) in self.targets.iter().enumerate() {
                self.write_set(src.sets[i], src.view, target);
                self.record(src.cmds[i], src.sets[i], src.image, target, false)?;
            }

            let pool = d.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(self.queue_family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let cmd = d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(pool).command_buffer_count(1))?[0];
            self.copy = Some(CopyTarget { pool, cmd, image: src.image, width, height });
        }
        self.sources.push(src);
        Ok(())
    }

    /// Copies `from` (in TRANSFER_SRC_OPTIMAL, the region starting at
    /// `origin`) into our image and converts it into target `index`.
    /// Returns how long the conversion shader ran.
    pub fn convert_copied(&mut self, from: vk::Image, origin: (i32, i32), index: usize) -> Result<std::time::Duration> {
        let copy = self.copy.as_ref().context("no copy source set up")?;
        let (cmd, to, width, height) = (copy.cmd, copy.image, copy.width, copy.height);
        let d = &self.device;
        unsafe {
            d.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            d.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))?;
            let to_dst = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(to)
                .subresource_range(color_range());
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_dst],
            );
            let layers = vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            };
            let region = vk::ImageCopy::default()
                .src_subresource(layers)
                .src_offset(vk::Offset3D { x: origin.0, y: origin.1, z: 0 })
                .dst_subresource(layers)
                .extent(vk::Extent3D { width, height, depth: 1 });
            d.cmd_copy_image(cmd, from, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, to, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
            let to_general = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(to)
                .subresource_range(color_range());
            d.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_general],
            );
            d.end_command_buffer(cmd)?;
        }
        let convert = self
            .sources
            .iter()
            .find(|s| s.fb_id == COPY_SOURCE)
            .context("no copy source set up")?
            .cmds[index];
        self.submit(&[cmd, convert])
    }
}
