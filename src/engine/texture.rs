use half::f16;

pub struct Tex {
    pub texture: wgpu::Texture,
    /// View over every mip level.
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
    pub mips: u32,
}

pub fn mip_count(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

impl Tex {
    pub fn new(
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> Self {
        Self::create(device, label, width, height, format, usage, 1, 1)
    }

    pub fn with_samples(
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        samples: u32,
    ) -> Self {
        Self::create(device, label, width, height, format, usage, samples, 1)
    }

    pub fn with_mips(
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> Self {
        Self::create(device, label, width, height, format, usage, 1, mip_count(width, height))
    }

    #[allow(clippy::too_many_arguments)]
    fn create(
        device: &wgpu::Device,
        label: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        samples: u32,
        mips: u32,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, width, height, format, usage, mips }
    }

    pub fn mip_view(&self, level: u32) -> wgpu::TextureView {
        self.texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        })
    }

    pub fn bytes_per_pixel(&self) -> u32 {
        bytes_per_pixel(self.format)
    }

    /// Tightly packed rows, mip 0.
    pub fn write_bytes(&self, queue: &wgpu::Queue, data: &[u8]) {
        self.write_mip(queue, 0, data);
    }

    pub fn write_mip(&self, queue: &wgpu::Queue, mip: u32, data: &[u8]) {
        let w = (self.width >> mip).max(1);
        let h = (self.height >> mip).max(1);
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(w * self.bytes_per_pixel()), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Blocking readback of mip 0, tightly packed rows. Needs COPY_SRC.
    pub fn read_bytes(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        self.read_region(device, queue, 0, 0, self.width, self.height)
    }

    pub fn read_region(&self, device: &wgpu::Device, queue: &wgpu::Queue, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let bpp = self.bytes_per_pixel();
        let row = w * bpp;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(Some(enc.finish()));
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().expect("map callback").expect("map buffer");
        let mapped = slice.get_mapped_range();
        let mut out = Vec::with_capacity((row * h) as usize);
        for yy in 0..h {
            let start = (yy * padded) as usize;
            out.extend_from_slice(&mapped[start..start + row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        out
    }

    /// One rgba16f texel as f32.
    pub fn read_texel_f16(&self, device: &wgpu::Device, queue: &wgpu::Queue, x: u32, y: u32) -> [f32; 4] {
        let b = self.read_region(device, queue, x.min(self.width - 1), y.min(self.height - 1), 1, 1);
        let v = f16_bytes_to_f32(&b);
        [v[0], *v.get(1).unwrap_or(&0.0), *v.get(2).unwrap_or(&0.0), *v.get(3).unwrap_or(&0.0)]
    }

    /// Same size/format/usage, contents of mip 0 copied. Needs COPY_SRC and COPY_DST.
    pub fn duplicate(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, label: &str) -> Tex {
        let t = Tex::create(device, label, self.width, self.height, self.format, self.usage, 1, self.mips);
        self.copy_to(encoder, &t);
        t
    }

    pub fn copy_to(&self, encoder: &mut wgpu::CommandEncoder, dst: &Tex) {
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            dst.texture.as_image_copy(),
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
    }

    pub fn byte_size(&self) -> u64 {
        let base = self.width as u64 * self.height as u64 * self.bytes_per_pixel() as u64;
        if self.mips > 1 {
            base * 4 / 3
        } else {
            base
        }
    }
}

pub fn bytes_per_pixel(format: wgpu::TextureFormat) -> u32 {
    use wgpu::TextureFormat as F;
    match format {
        F::Rgba16Float | F::Rg32Uint => 8,
        F::R16Float => 2,
        F::Rg16Float | F::Rg16Uint | F::Rgba8Unorm | F::Rgba8UnormSrgb | F::Bgra8Unorm | F::R32Float | F::Depth32Float => 4,
        F::Rgba32Float => 16,
        F::R8Unorm => 1,
        other => panic!("unsupported format {other:?}"),
    }
}

pub fn f32_to_f16_bytes(data: &[f32]) -> Vec<u8> {
    use rayon::prelude::*;
    let mut out = vec![0u8; data.len() * 2];
    out.par_chunks_mut(2 * 65536).zip(data.par_chunks(65536)).for_each(|(o, d)| {
        for (o, &v) in o.chunks_exact_mut(2).zip(d) {
            o.copy_from_slice(&f16::from_f32(v).to_le_bytes());
        }
    });
    out
}

pub fn f16_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2).map(|c| f16::from_le_bytes([c[0], c[1]]).to_f32()).collect()
}

pub fn f32_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// Uploads an 8-bit RGBA image with a CPU-built mip chain.
pub fn rgba8_with_mips(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    img: &image::RgbaImage,
) -> Tex {
    let (w, h) = img.dimensions();
    let t = Tex::with_mips(
        device,
        label,
        w,
        h,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
    );
    t.write_mip(queue, 0, img.as_raw());
    let mut cur = img.clone();
    for mip in 1..t.mips {
        let nw = (w >> mip).max(1);
        let nh = (h >> mip).max(1);
        cur = image::imageops::resize(&cur, nw, nh, image::imageops::FilterType::Triangle);
        t.write_mip(queue, mip, cur.as_raw());
    }
    t
}
