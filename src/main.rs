use std::{
    borrow::Cow,
    fs::File,
    io::Read,
    mem,
    path::Path,
    str::FromStr,
    time::{Duration, Instant},
};

use emulator::{EmulationDesc, Emulator, RunState, DISPLAY_SIZE};
use image::GenericImageView;
use imgui::FontSource;
use imgui_wgpu::{Renderer, RendererConfig};
use wgpu::{
    util::{BufferInitDescriptor, DeviceExt},
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BufferAddress, BufferUsages, Color, ImageDataLayout, Origin3d, SamplerDescriptor,
    ShaderLocation, ShaderStages, TextureUsages, TextureView, TextureViewDescriptor,
};
use winit::{
    dpi::LogicalSize,
    event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

mod emulator;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pub pos: [f32; 3],
    pub uvs: [f32; 2],
}

const RGBA_BLACK: [u8; 4] = [0, 0, 0, 255];
const RGBA_WHITE: [u8; 4] = [255, 255, 255, 255];

fn main() {
    env_logger::init();
    let eloop = EventLoop::new();
    let wnd = winit::window::Window::new(&eloop).expect("Error creating window.");
    wnd.set_inner_size(LogicalSize {
        width: 1280.0,
        height: 720.0,
    });
    pollster::block_on(run(eloop, wnd));
}

async fn run(event_loop: EventLoop<()>, wnd: Window) {
    let size = wnd.inner_size();
    let wgpu = wgpu::Instance::new(wgpu::Backends::all());
    let surface = unsafe { wgpu.create_surface(&wnd) };
    let adapter = wgpu
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        })
        .await
        .expect("Failed to find an adapter.");

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
            },
            None,
        )
        .await
        .expect("Failed to create device");

    let mut shader_source = String::new();
    File::open(Path::new(
        "D:/Development/Rust Crates/chip_8_emulator/resources/shader.wgsl",
    ))
    .expect("Issue finding shader file.")
    .read_to_string(&mut shader_source)
    .expect("Issue reading source file.");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source.as_str())),
    });

    let mut texture_data: [[u8; 4]; DISPLAY_SIZE.1 * DISPLAY_SIZE.0] =
        [RGBA_BLACK; DISPLAY_SIZE.1 * DISPLAY_SIZE.0];
    let texture_size = wgpu::Extent3d {
        width: DISPLAY_SIZE.0 as u32,
        height: DISPLAY_SIZE.1 as u32,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        label: Some("CHIP-8 Display diffuse"),
    });
    let texture_view = texture.create_view(&TextureViewDescriptor::default());
    let texture_sampler = device.create_sampler(&SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let texture_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
        label: None,
    });
    let texture_bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &texture_bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&texture_sampler),
            },
        ],
    });

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("CHIP-8 Vertex buffer"),
        contents: bytemuck::cast_slice(&[
            Vertex {
                pos: [1.0, 1.0, 0.0],
                uvs: [1.0, 0.0],
            },
            Vertex {
                pos: [-1.0, -1.0, 0.0],
                uvs: [0.0, 1.0],
            },
            Vertex {
                pos: [1.0, -1.0, 0.0],
                uvs: [1.0, 1.0],
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0],
                uvs: [0.0, 0.0],
            },
        ]),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&BufferInitDescriptor {
        label: Some("CHIP-8 Index buffer"),
        contents: bytemuck::cast_slice(&[0u16, 1, 2, 1, 0, 3]),
        usage: BufferUsages::INDEX,
    });

    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: mem::size_of::<[f32; 3]>() as BufferAddress,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    };

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&texture_bind_group_layout],
        push_constant_ranges: &[],
    });

    let swapchain_format = surface.get_supported_formats(&adapter)[0];

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[vertex_layout],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(swapchain_format.into())],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface.get_supported_formats(&adapter)[0],
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
    };

    surface.configure(&device, &config);

    let hidpi_factor = wnd.scale_factor();

    let mut imgui = imgui::Context::create();
    let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui);
    platform.attach_window(
        imgui.io_mut(),
        &wnd,
        imgui_winit_support::HiDpiMode::Default,
    );
    imgui.set_ini_filename(Some(
        Path::new("D:/Development/Rust Crates/chip_8_emulator/imgui.ini").to_path_buf(),
    ));
    imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;
    imgui.fonts().add_font(&[FontSource::DefaultFontData {
        config: Some(imgui::FontConfig {
            oversample_h: 1,
            pixel_snap_h: true,
            size_pixels: (13.0 * wnd.scale_factor()) as f32,
            ..Default::default()
        }),
    }]);
    let renderer_config = RendererConfig {
        texture_format: config.format,
        ..Default::default()
    };
    let mut renderer = Renderer::new(&mut imgui, &device, &queue, renderer_config);

    let mut emulator = Emulator::new(EmulationDesc::default());
    emulator.load_font();
    emulator.load_rom(
        String::from_str("D:/Development/Rust Crates/chip_8_emulator/resources/roms/Trip8 Demo (2008) [Revival Studios].ch8")
            .unwrap(),
    );

    // for i in 0..DISPLAY_SIZE.0 {
    //     emulator.display[i][0] = 1;
    // }
    // for i in 0..DISPLAY_SIZE.1 {
    //     emulator.display[31][i] = 1;
    // }

    let mut last_frame = Instant::now();
    let mut last_cursor = None;

    event_loop.run(move |event, _, flow| {
        let _ = (&wgpu, &adapter, &shader, &pipeline_layout);
        wnd.request_redraw();
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == wnd.id() => match event {
                WindowEvent::CloseRequested => *flow = ControlFlow::Exit,
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            state: pressed,
                            scancode: key,
                            ..
                        },
                    ..
                } => {
                    if let ElementState::Released = pressed {
                        emulator.key = None;
                    } else {
                        if let Some(mapped) = map_key(*key) {
                            emulator.key = Some(mapped);
                        }
                    }
                }
                WindowEvent::Resized(size) => {
                    config.width = size.width;
                    config.height = size.height;
                    surface.configure(&device, &config);
                    wnd.request_redraw();
                }
                _ => {}
            },
            Event::RedrawRequested(_) => {
                let start_time = Instant::now();
                imgui.io_mut().update_delta_time(start_time - last_frame);
                last_frame = start_time;

                let frame = surface
                    .get_current_texture()
                    .expect("Failed to acquire next swap chain texture");
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                platform
                    .prepare_frame(imgui.io_mut(), &wnd)
                    .expect("Failed to prepare frame.");
                let ui = imgui.frame();

                if let RunState::Running = emulator.state {
                    emulator.step();
                }
                emulator.draw_info(ui);

                for x in 0..DISPLAY_SIZE.0 {
                    for y in 0..DISPLAY_SIZE.1 {
                        texture_data[(y * DISPLAY_SIZE.0) + x] = if emulator.display[x][y] == 1 {
                            RGBA_WHITE
                        } else {
                            RGBA_BLACK
                        };
                    }
                }
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &texture,
                        mip_level: 0,
                        origin: Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&texture_data),
                    ImageDataLayout {
                        offset: 0,
                        bytes_per_row: std::num::NonZeroU32::new(4 * DISPLAY_SIZE.0 as u32),
                        rows_per_image: std::num::NonZeroU32::new(DISPLAY_SIZE.1 as u32),
                    },
                    texture_size,
                );

                if last_cursor != Some(ui.mouse_cursor()) {
                    last_cursor = Some(ui.mouse_cursor());
                    platform.prepare_render(&ui, &wnd);
                }

                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: true,
                            },
                        })],
                        depth_stencil_attachment: None,
                    });
                    rpass.set_pipeline(&render_pipeline);
                    rpass.set_bind_group(0, &texture_bind_group, &[]);
                    rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    rpass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    rpass.draw_indexed(0..6, 0, 0..1);

                    renderer
                        .render(imgui.render(), &queue, &device, &mut rpass)
                        .expect("Render failed.");
                }

                queue.submit(Some(encoder.finish()));
                frame.present();

                let elapsed_time = Instant::now().duration_since(start_time).as_millis() as u64;
                let wait_time = if 1000 / 60 >= elapsed_time {
                    1000 / 60 - elapsed_time
                } else {
                    0
                };
                let wait_instant = start_time + Duration::from_millis(wait_time);
                *flow = ControlFlow::WaitUntil(wait_instant)
            }
            _ => {}
        }

        platform.handle_event(imgui.io_mut(), &wnd, &event);
    });
}

fn map_key(scancode: u32) -> Option<u8> {
    match scancode {
        0x2 => Some(0x1),
        0x3 => Some(0x2),
        0x4 => Some(0x3),
        0x5 => Some(0xC),
        0x10 => Some(0x4),
        0x11 => Some(0x5),
        0x12 => Some(0x6),
        0x13 => Some(0xD),
        0x1E => Some(0x7),
        0x1F => Some(0x8),
        0x20 => Some(0x9),
        0x21 => Some(0xE),
        0x2C => Some(0xA),
        0x2D => Some(0x0),
        0x2E => Some(0xB),
        0x3F => Some(0xF),
        _ => None,
    }
}
