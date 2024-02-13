use std::num::NonZeroU32;

use anyhow::*;
use image::{DynamicImage, GenericImageView, Rgba};
use wgpu::{Device, TextureDescriptor, util::DeviceExt};
use winapi::um::d2d1effects::CLSID_D2D1DisplacementMap;

pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

impl Texture {
    pub fn from_rgba_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba8_bytes: &image::RgbaImage,
        label: &str,
    ) -> Result<Self> {
        let img = image::load_from_memory(rgba8_bytes)?;
        Self::from_image(device, queue, &img, Some(label))
    }

    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &image::DynamicImage,
        label: Option<&str>,
    ) -> Result<Self> {
        let dimensions = img.dimensions();

        let extent_size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };
        // create a texture so we can write to it
        let texture_surface: wgpu::Texture = Self::create_output_texture(
            device,
            extent_size.width as u16,
            extent_size.height as u16,
        );

        println!("from_image: 1");

        let rgba_4x8_image: image::RgbaImage = img.to_rgba8();
        println!("from_image: 2");
        queue.write_texture(
            wgpu::ImageCopyTexture {
                aspect: wgpu::TextureAspect::All,
                texture: &texture_surface,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &rgba_4x8_image,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),  // R, G, B, A => 4 bytes per pixel
                rows_per_image: Some(dimensions.1),
            },
            extent_size,
        );
        println!("from_image: 3");

        let view = texture_surface.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        println!("from_image: 4");
        Ok(Self {
            texture: texture_surface,
            view,
            sampler,
        })
    }

    fn create_texture_from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &DynamicImage,
    ) -> wgpu::Texture {
        let rgba_data = img.to_rgba8().into_raw();

        // Create a wgpu buffer from the image data
        let buffer: wgpu::Buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Texture Buffer"),
            contents: bytemuck::cast_slice(&rgba_data),
            usage: wgpu::BufferUsages::COPY_SRC,
        });

        // Create a wgpu texture from the buffer
        let texture: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: img.width(),
                height: img.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Copy the buffer data to the texture
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * img.width()),
                    rows_per_image: Some(img.height()),
                },
            },
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: img.width(),
                height: img.height(),
                depth_or_array_layers: 1,
            },
        );

        // Submit the copy operation
        queue.submit(Some(encoder.finish()));

        texture
    }

    fn create_output_texture(device: &wgpu::Device, width: u16, height: u16) -> wgpu::Texture {
        let texture_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        //let texture = device.create_texture(&wgpu::TextureDescriptor {
        //    label,
        //    extent_size ,
        //    mip_level_count: 1,
        //    sample_count: 1,
        //    dimension: wgpu::TextureDimension::D2,
        //    format,
        //    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        //    view_formats: &[],
        //});
        let texture_create_usage = wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT;
        // Create a wgpu texture for output (you might need to adjust the format and dimensions)
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: width as u32,
                height: height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            //usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            usage: texture_create_usage,
            view_formats: &[],
        })
    }
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, Rgba};
    use std::num::NonZeroU32;
    use wgpu::{
        util::DeviceExt, CommandEncoder, Device, Texture, TextureDescriptor, TextureUsages,
    };

    //fn main() {
    //    // Load your raw RGBA32 image (replace with your file path or data loading logic)
    //    let image_path = "path/to/your/image.png";
    //    let img = image::open(image_path).expect("Failed to open image").to_rgba8();

    //    // Set up wgpu device and queue (you might need to initialize these properly based on your project)
    //    let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
    //    let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
    //        power_preference: wgpu::PowerPreference::Default,
    //        compatible_surface: None,
    //    }).expect("Failed to find an appropriate adapter");

    //    let (device, queue) = futures::executor::block_on(async {
    //        adapter.request_device(
    //            &wgpu::DeviceDescriptor {
    //                label: None,
    //                features: wgpu::Features::empty(),
    //                limits: wgpu::Limits::default(),
    //            },
    //            None,
    //        )
    //    }).expect("Failed to create device");

    //    // Create a wgpu texture from the loaded image
    //    let texture = create_texture_from_image(&device, &img);

    //    // Perform rendering operations using the texture (replace with your rendering logic)
    //    // ...

    //    // For the sake of this example, we will encode a simple copy operation to a render target
    //    let output_texture = create_output_texture(&device);

    //    // Create a command encoder and perform the copy operation
    //    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    //    encoder.copy_texture_to_texture(
    //        wgpu::ImageCopyTexture {
    //            texture: &texture,
    //            mip_level: 0,
    //            origin: wgpu::Origin3d::ZERO,
    //        },
    //        wgpu::ImageCopyTexture {
    //            texture: &output_texture,
    //            mip_level: 0,
    //            origin: wgpu::Origin3d::ZERO,
    //        },
    //        wgpu::Extent3d {
    //            width: img.width(),
    //            height: img.height(),
    //            depth_or_array_layers: 1,
    //        },
    //    );

    //    // Submit the command encoder for execution
    //    queue.submit(Some(encoder.finish()));
    //}
}
