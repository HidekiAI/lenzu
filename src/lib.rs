mod my_desktop;
mod my_ocr;
mod my_texture;

use std::iter;

use my_desktop::Desktop;
use screenshots::{self, display_info::DisplayInfo, Screen};
use wgpu::util::DeviceExt;
use winit::{
    event::*,
    //event::{Event, WindowEvent},
    //platform::desktop::WindowBuilderExtDesktop,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1.0, 1.0, 0.0],
        tex_coords: [0.0, 0.0],
    }, // Top-left
    Vertex {
        position: [1.0, 1.0, 0.0],
        tex_coords: [1.0, 0.0],
    }, // Top-right
    Vertex {
        position: [-1.0, -1.0, 0.0],
        tex_coords: [0.0, 1.0],
    }, // Bottom-left
    Vertex {
        position: [1.0, -1.0, 0.0],
        tex_coords: [1.0, 1.0],
    }, // Bottom-right
];

const INDICES: &[u16] = &[0, 1, 2, 1, 3, 2];

struct State {
    // NOTE: Even if there are more than one monitors, the window is based on the monitor that it's (created) on.
    //       But once it's created, we mostly care about the surface we're rendering to, and (hopefully) even if
    //       the window is moved to another monitor, it will render on the surface of the monitor that it's on.
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    window_dimension: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    #[allow(unused_variables)]
    #[allow(dead_code)]
    texture: my_texture::Texture, // for bind_group usage
    bind_group: wgpu::BindGroup, // for update
    needs_update: bool,          //if the texture (image) changed, we need to update the bind_group

    // for mouse location related (with respect to the (possible) multiple screen)
    mouse_data: my_desktop::Desktop,

    // somewhere I read that Window has to be the tail of the struct, so I'm putting it here
    window: Window,
}

impl State {
    fn get_image() -> &'static [u8] {
        include_bytes!("sample.png")
    }
    async fn new(window1: Window) -> Self {
        let window_size = window1.inner_size();

        // The instance is a handle to our GPU
        // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            dx12_shader_compiler: Default::default(),
        });

        // The surface needs to live as long as the window that created it.
        // State owns the window so this should be safe.
        let surface1 = unsafe { instance.create_surface(&window1) }.unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface1),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device1, queue1) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    // WebGL doesn't support all of wgpu's features, so if
                    // we're building for the web we'll have to disable some.
                    limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults()
                    } else {
                        wgpu::Limits::default()
                    },
                },
                None, // Trace path
            )
            .await
            .unwrap();

        let surface_caps = surface1.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an Srgb surface texture. Using a different
        // one will result all the colors comming out darker. If you want to support non
        // Srgb surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config1 = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: window_size.width,
            height: window_size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface1.configure(&device1, &config1);

        let image_bytes1: &[u8] = Self::get_image();
        let texture1: my_texture::Texture =
            my_texture::Texture::from_bytes(&device1, &queue1, image_bytes1, "diffuse_bytes")
                .unwrap();

        let texture_bind_group_layout =
            device1.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let bind_group1 = device1.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture1.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&texture1.sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        let shader = device1.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout =
            device1.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline1 = device1.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config1.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent::REPLACE,
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
                // or Features::POLYGON_MODE_POINT
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            // If the pipeline will be used with a multiview render pass, this
            // indicates how many array layers the attachments will have.
            multiview: None,
        });

        let vertex_buffer1 = device1.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer1 = device1.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });
        let num_indices1 = INDICES.len() as u32;

        let monitors: Vec<winit::monitor::MonitorHandle> = window1.available_monitors().collect();
        let mut screens = Vec::new();
        for (index, monitor) in monitors.iter().enumerate() {
            let screen = my_desktop::Screen::new(
                index as u8,
                monitor.size().width,
                monitor.size().height,
                monitor.position().x,
                monitor.position().y,
            );
            screens.push(screen);
        }
        Self {
            surface: surface1,
            device: device1,
            queue: queue1,
            config: config1,
            render_pipeline: render_pipeline1,
            vertex_buffer: vertex_buffer1,
            index_buffer: index_buffer1,
            num_indices: num_indices1,
            texture: texture1,
            bind_group: bind_group1,
            window_dimension: window_size,
            needs_update: false,
            mouse_data: Desktop::new(screens.as_slice()),
            window: window1,
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.window_dimension = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    #[allow(unused_variables)]
    fn input(&mut self, event: &WindowEvent) -> bool {
        //false
        match event {
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        state,
                        virtual_keycode: Some(VirtualKeyCode::Space),
                        ..
                    },
                ..
            } => {
                //self.is_space_pressed = *state == ElementState::Pressed;
                true
            }
            _ => false,
        }
    }

    // Function to update the wgpu texture with the new frame data
    fn update(&mut self) {
        if self.needs_update {
            let image_bytes1: &[u8] = Self::get_image();

            // Map the texture for writing
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("update_encoder"),
                });
            //let mut staging_buffer =
            //    device.create_buffer_with_data(frame.data(0).unwrap(), wgpu::BufferUsage::COPY_SRC);
            //let mut texture_buffer = device.create_buffer_with_data(
            //    &[0u8; 4], // Placeholder data, you need to replace this with actual texture data
            //    wgpu::BufferUsage::COPY_DST | wgpu::BufferUsage::MAP_WRITE,
            //);

            //// Copy the frame data to the texture buffer
            //encoder.copy_buffer_to_buffer(
            //    &staging_buffer,
            //    0,
            //    &texture_buffer,
            //    0,
            //    frame.data(0).unwrap().len() as wgpu::BufferAddress,
            //);

            //// Unmap the texture buffer
            //let texture_mapping = texture_buffer.map_write();
            //let texture_data = texture_mapping.get_mapped_range().to_owned();
            //texture_buffer.unmap();

            //// Update the wgpu texture with the new data
            //let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            //let texture_extent = wgpu::Extent3d {
            //    width: frame.width() as u32,
            //    height: frame.height() as u32,
            //    depth_or_array_layers: 1,
            //};

            //encoder.copy_buffer_to_texture(
            //    wgpu::ImageCopyBuffer {
            //        buffer: &texture_buffer,
            //        layout: wgpu::ImageDataLayout {
            //            offset: 0,
            //            bytes_per_row: Some(frame.linesize(0) as u32),
            //            rows_per_image: None,
            //        },
            //    },
            //    wgpu::ImageCopyTexture {
            //        texture: &texture_view,
            //        mip_level: 0,
            //        origin: wgpu::Origin3d::ZERO,
            //    },
            //    texture_extent,
            //);

            //// Submit the command encoder to the queue
            //device.get_queue().submit(Some(encoder.finish()));
            self.needs_update = false;
        }
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            //let bind_group = if self.is_space_pressed {
            //    &self.bind_group2
            //} else {
            //    &self.bind_group
            //};
            //render_pass.set_bind_group(0, bind_group, &[]);
            render_pass.set_bind_group(0, &self.bind_group, &[]);

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
        }

        self.queue.submit(iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
    fn render2(&mut self) -> Result<(), wgpu::SurfaceError> {
        let mut encoder: wgpu::CommandEncoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("render_encoder"),
                });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass_descriptor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.texture.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            // Implement binding the texture to a bind group and setting up the pipeline
            // ...

            // this method, we render based on fixed verts (uses draw() instead of draw_indexed())
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn update_screen_info(&mut self, cursor_position_in_window: winit::dpi::PhysicalPosition<f64>) -> my_desktop::Screen {
        // Cursor is within this monitor
        let screen_where_mouse_resides = self.mouse_data.update();

        screen_where_mouse_resides
    }

    // Assume that you have a wgpu device, queue, swap_chain, and shader modules already set up.
    // Function to create a wgpu texture for video frames
    // use screenshots to capture screen where - assume this gets called AFTER get_screen_info is called
    fn take_screenshot_of_monitor(&self) -> Option<Vec<u8>> {
        // locate the correct DisplayInfo
        for display in DisplayInfo::all().unwrap() {
            if display.id == self.mouse_data.current_screen().index as u32 {
                let screen = Screen::new(&display);

                let (x, y, width, height) = self.get_capture_rect();

                // Take a screenshot of the monitor
                // image::ImageBuffer<image::Rgba<u8>, Vec<u8>>
                let possible_screenshot = screen.capture_area(x, y, width, height);
                match possible_screenshot {
                    Ok(screenshot) => {
                        let rgba_image: image::RgbaImage = screenshot;
                        let rgba32 = rgba_image.into_raw();
                        return Some(rgba32);
                    }
                    Err(e) => {
                        println!("Error: {}", e);
                        return None;
                    }
                }
            }
        }

        None
    }

    // based on current screen data, calculate the capture rect
    fn get_capture_rect(&self) -> (i32, i32, u32, u32) {
        let rect_width = 512;
        let rect_height = 512;
        // assume current position is center of the rect, but if it goes out of the screen, adjust it
        let mut upper_left_x =
            self.mouse_data.current_screen().top_x as i32 - rect_width as i32 / 2;
        let mut upper_left_y =
            self.mouse_data.current_screen().top_y as i32 - rect_height as i32 / 2;

        // if left or right edges are out of the screen, adjust upper_left_x
        if upper_left_x < 0 {
            upper_left_x = 0;
        } else if upper_left_x + rect_width as i32 > self.mouse_data.current_screen().width as i32 {
            upper_left_x = self.mouse_data.current_screen().width as i32 - rect_width as i32;
        }
        // if top or bottom edges are out of the screen, adjust upper_left_y
        if upper_left_y < 0 {
            upper_left_y = 0;
        } else if upper_left_y + rect_height as i32 > self.mouse_data.current_screen().height as i32
        {
            upper_left_y = self.mouse_data.current_screen().height as i32 - rect_height as i32;
        }

        (upper_left_x, upper_left_y, rect_width, rect_height)
    }
}

//fn main() {
//    pollster::block_on(run());
//}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub async fn run() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Warn).expect("Could't initialize logger");
        } else {
            env_logger::init();
        }
    }

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    #[cfg(target_arch = "wasm32")]
    {
        // Winit prevents sizing with CSS, so we have to set
        // the size manually when on web.
        use winit::dpi::PhysicalSize;
        window.set_inner_size(PhysicalSize::new(450, 400));

        use winit::platform::web::WindowExtWebSys;
        web_sys::window()
            .and_then(|win| win.document())
            .and_then(|doc| {
                let dst = doc.get_element_by_id("wasm-example")?;
                let canvas = web_sys::Element::from(window.canvas());
                dst.append_child(&canvas).ok()?;
                Some(())
            })
            .expect("Couldn't append canvas to document body.");
    }

    // State::new uses async code, so we're going to wait for it to finish
    let mut state = State::new(window).await;

    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == state.window().id() => {
                if !state.input(event) {
                    match event {
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            input:
                                KeyboardInput {
                                    state: ElementState::Pressed,
                                    virtual_keycode: Some(VirtualKeyCode::Escape),
                                    ..
                                },
                            ..
                        } => *control_flow = ControlFlow::Exit,
                        WindowEvent::CursorMoved {
                            device_id,
                            position,
                            modifiers,
                        } => {
                            let screen_info = state.update_screen_info(*position);
                            println!(
                                "Screen {}: {}x{}, Cursor Position: ({}, {})",
                                screen_info.index,
                                screen_info.width,
                                screen_info.height,
                                screen_info.top_x,
                                screen_info.top_y
                            );
                        }
                        WindowEvent::Resized(physical_size) => {
                            state.resize(*physical_size);
                        }
                        WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                            // new_inner_size is &mut so w have to dereference it twice
                            state.resize(**new_inner_size);
                        }
                        _ => {}
                    }
                }
            }
            Event::RedrawRequested(window_id) if window_id == state.window().id() => {
                state.update();
                match state.render() {
                    Ok(_) => {}
                    // Reconfigure the surface if it's lost or outdated
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state.resize(state.window_dimension)
                    }
                    // The system is out of memory, we should probably quit
                    Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                    // We're ignoring timeouts
                    Err(wgpu::SurfaceError::Timeout) => log::warn!("Surface timeout"),
                }
            }
            Event::MainEventsCleared => {
                // RedrawRequested will only trigger once, unless we manually
                // request it.
                state.window().request_redraw();
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[tokio::test]
    async fn test_mouse_pos_screen_info() {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new().build(&event_loop).unwrap();

        // State::new uses async code, so we're going to wait for it to finish
        let mut state = State::new(window).await;

        let cursor_position = winit::dpi::PhysicalPosition::new(100.0, 100.0);
        let screen_info = state.update_screen_info(cursor_position);
        println!(
            "Screen {}: {}x{}, Cursor Position: ({}, {})",
            screen_info.index,  
            screen_info.width,
            screen_info.height,
            screen_info.top_x,
            screen_info.top_y
        );
    }
}
