//! wgpu viewport renderer. Draws a [`Scene`] into an offscreen, multisampled texture that the
//! UI shows as an image. Rendering offscreen gives the viewport its own depth buffer and MSAA,
//! independent of egui's render pass.

mod scene;

pub use scene::{Color, DashedVertex, FaceVertex, LineVertex, Scene, SceneGuide, guide_vertices, srgb};

use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

/// The format egui samples registered textures in.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLE_COUNT: u32 = 4;
/// Size of the `Globals` uniform: the view-projection matrix, the eye position (a vec4), the
/// inverse view-projection matrix and the viewport (pixels and pixels per point).
const GLOBALS_SIZE: u64 = 160;

pub struct ViewportRenderer {
    sky_pipeline: wgpu::RenderPipeline,
    background_line_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    face_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    background_lines: GpuBuffer,
    lines: GpuBuffer,
    faces: GpuBuffer,
    dashed_pipeline: wgpu::RenderPipeline,
    /// Guides in world coordinates, made into dashed lines each frame for the camera.
    guides: Vec<SceneGuide>,
    selected_face_pipeline: wgpu::RenderPipeline,
    selected_faces: GpuBuffer,
    selected_lines: GpuBuffer,
    selected_guides: Vec<SceneGuide>,
    targets: Option<Targets>,
    background: wgpu::Color,
}

struct GpuBuffer {
    buffer: wgpu::Buffer,
    vertex_count: u32,
}

/// Size-dependent textures, recreated when the viewport is resized.
struct Targets {
    size: [u32; 2],
    msaa_color: wgpu::TextureView,
    depth: wgpu::TextureView,
    output: wgpu::TextureView,
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device, scene: &Scene) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport globals"),
            size: GLOBALS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport globals"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let line_spec = |label, depth_write| PipelineSpec {
            label,
            vertex_entry: "vs_line",
            fragment_entry: "fs_line",
            stride: size_of::<LineVertex>(),
            attributes: &line_attributes,
            topology: wgpu::PrimitiveTopology::LineList,
            // Edges drawn exactly on a face must win the depth test against it.
            depth_compare: wgpu::CompareFunction::LessEqual,
            depth_write,
            depth_bias: wgpu::DepthBiasState::default(),
        };
        let line_pipeline = create_pipeline(device, &layout, &shader, line_spec("viewport lines", true));
        let background_line_pipeline =
            create_pipeline(device, &layout, &shader, line_spec("viewport background lines", false));

        let face_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4
        ];
        let dashed_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x3, 3 => Float32];
        let dashed_pipeline = create_pipeline(
            device,
            &layout,
            &shader,
            PipelineSpec {
                label: "viewport guides",
                vertex_entry: "vs_dashed",
                fragment_entry: "fs_dashed",
                stride: size_of::<DashedVertex>(),
                attributes: &dashed_attributes,
                topology: wgpu::PrimitiveTopology::LineList,
                // Faces hide guides; a guide lying on a face still shows (faces are biased back).
                depth_compare: wgpu::CompareFunction::LessEqual,
                depth_write: false,
                depth_bias: wgpu::DepthBiasState::default(),
            },
        );
        let face_pipeline = create_pipeline(
            device,
            &layout,
            &shader,
            PipelineSpec {
                label: "viewport faces",
                vertex_entry: "vs_face",
                fragment_entry: "fs_face",
                stride: size_of::<FaceVertex>(),
                attributes: &face_attributes,
                topology: wgpu::PrimitiveTopology::TriangleList,
                depth_compare: wgpu::CompareFunction::Less,
                depth_write: true,
                // Push faces slightly back so their own edges never z-fight with them.
                depth_bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            },
        );

        // The selection's tinted faces, redrawn over the same triangles: equal depth passes.
        let selected_face_pipeline = create_pipeline(
            device,
            &layout,
            &shader,
            PipelineSpec {
                label: "viewport selected faces",
                vertex_entry: "vs_face",
                fragment_entry: "fs_face",
                stride: size_of::<FaceVertex>(),
                attributes: &face_attributes,
                topology: wgpu::PrimitiveTopology::TriangleList,
                depth_compare: wgpu::CompareFunction::LessEqual,
                depth_write: false,
                depth_bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            },
        );

        let [r, g, b, _] = srgb(250, 250, 250);
        Self {
            selected_face_pipeline,
            selected_faces: GpuBuffer::new(device, "viewport selected faces", &scene.selected_faces),
            selected_lines: GpuBuffer::new(device, "viewport selected lines", &scene.selected_lines),
            selected_guides: scene.selected_guides.clone(),
            sky_pipeline: create_sky_pipeline(device, &layout, &shader),
            background_line_pipeline,
            line_pipeline,
            face_pipeline,
            globals,
            bind_group,
            background_lines: GpuBuffer::new(device, "viewport background lines", &scene.background_lines),
            lines: GpuBuffer::new(device, "viewport lines", &scene.lines),
            faces: GpuBuffer::new(device, "viewport faces", &scene.faces),
            dashed_pipeline,
            guides: scene.guides.clone(),
            targets: None,
            background: wgpu::Color { r: r.into(), g: g.into(), b: b.into(), a: 1.0 },
        }
    }

    /// Replace the geometry being drawn.
    pub fn set_scene(&mut self, device: &wgpu::Device, scene: &Scene) {
        self.background_lines = GpuBuffer::new(device, "viewport background lines", &scene.background_lines);
        self.lines = GpuBuffer::new(device, "viewport lines", &scene.lines);
        self.faces = GpuBuffer::new(device, "viewport faces", &scene.faces);
        self.guides = scene.guides.clone();
        self.set_selection(device, scene);
    }

    /// Replace just the selection highlight (`scene` built with [`Scene::add_selection`]):
    /// selecting doesn't rebuild the whole drawing.
    pub fn set_selection(&mut self, device: &wgpu::Device, scene: &Scene) {
        self.selected_faces = GpuBuffer::new(device, "viewport selected faces", &scene.selected_faces);
        self.selected_lines = GpuBuffer::new(device, "viewport selected lines", &scene.selected_lines);
        self.selected_guides = scene.selected_guides.clone();
    }

    /// Render one frame at `size` physical pixels. Returns `true` when the output texture was
    /// recreated, in which case the caller must re-register [`Self::output_view`] with egui.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        view_proj: Mat4,
        eye: Vec3,
        pixels_per_point: f32,
    ) -> bool {
        let size = [size[0].max(1), size[1].max(1)];
        let resized = self.targets.as_ref().is_none_or(|t| t.size != size);
        if resized {
            self.targets = Some(Targets::new(device, size));
        }
        let targets = self.targets.as_ref().expect("targets were just created");

        let mut globals = [0.0_f32; 40];
        globals[..16].copy_from_slice(&view_proj.to_cols_array());
        globals[16..19].copy_from_slice(&eye.to_array());
        globals[20..36].copy_from_slice(&view_proj.inverse().to_cols_array());
        globals[36..39].copy_from_slice(&[size[0] as f32, size[1] as f32, pixels_per_point]);
        let all_guides: Vec<SceneGuide> = self.guides.iter().chain(&self.selected_guides).copied().collect();
        let dashed = GpuBuffer::new(device, "viewport guides", &guide_vertices(&all_guides, view_proj, eye));
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&globals));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("viewport"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa_color,
                    depth_slice: None,
                    resolve_target: Some(&targets.output),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.background),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            for (pipeline, buffer) in [
                (&self.background_line_pipeline, &self.background_lines),
                (&self.face_pipeline, &self.faces),
                (&self.line_pipeline, &self.lines),
                (&self.selected_face_pipeline, &self.selected_faces),
                (&self.line_pipeline, &self.selected_lines),
                (&self.dashed_pipeline, &dashed),
            ] {
                if buffer.vertex_count > 0 {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, buffer.buffer.slice(..));
                    pass.draw(0..buffer.vertex_count, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);
        resized
    }

    /// The resolved frame, ready to sample. `None` before the first [`Self::render`].
    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.output)
    }
}

impl GpuBuffer {
    fn new<const N: usize>(device: &wgpu::Device, label: &str, vertices: &[[f32; N]]) -> Self {
        Self {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                // wgpu rejects zero-sized vertex buffers, so pad an empty scene.
                contents: if vertices.is_empty() {
                    &[0; 4]
                } else {
                    bytemuck::cast_slice(vertices)
                },
                usage: wgpu::BufferUsages::VERTEX,
            }),
            vertex_count: vertices.len() as u32,
        }
    }
}

impl Targets {
    fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let texture = |label, format, sample_count, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        Self {
            size,
            msaa_color: texture(
                "viewport msaa color",
                OUTPUT_FORMAT,
                SAMPLE_COUNT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            depth: texture(
                "viewport depth",
                DEPTH_FORMAT,
                SAMPLE_COUNT,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            output: texture(
                "viewport output",
                OUTPUT_FORMAT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        }
    }
}

struct PipelineSpec<'a> {
    label: &'a str,
    vertex_entry: &'a str,
    fragment_entry: &'a str,
    stride: usize,
    attributes: &'a [wgpu::VertexAttribute],
    topology: wgpu::PrimitiveTopology,
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
    depth_bias: wgpu::DepthBiasState,
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    spec: PipelineSpec<'_>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(spec.vertex_entry),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: spec.stride as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: spec.attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: spec.topology,
            // SketchUp faces are two-sided.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(spec.depth_write),
            depth_compare: Some(spec.depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: spec.depth_bias,
        }),
        multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(spec.fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(OUTPUT_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The sky: one screen-covering triangle from the vertex index, behind everything.
fn create_sky_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("viewport sky"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_sky"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_sky"),
            compilation_options: Default::default(),
            targets: &[Some(OUTPUT_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}
