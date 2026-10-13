struct Globals {
    view_proj: mat4x4<f32>,
    // xyz: camera position, for telling front faces from back faces.
    eye: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct LineOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> LineOut {
    var out: LineOut;
    out.position = globals.view_proj * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    return in.color;
}

struct FaceOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) front: vec4<f32>,
    @location(3) back: vec4<f32>,
};

@vertex
fn vs_face(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) front: vec4<f32>,
    @location(3) back: vec4<f32>,
) -> FaceOut {
    var out: FaceOut;
    out.position = globals.view_proj * vec4<f32>(position, 1.0);
    out.world = position;
    out.normal = normal;
    out.front = front;
    out.back = back;
    return out;
}

@fragment
fn fs_face(in: FaceOut) -> @location(0) vec4<f32> {
    let view = normalize(in.world - globals.eye.xyz);
    let facing = dot(normalize(in.normal), view);
    // The normal points toward the viewer on the front side.
    let color = select(in.front, in.back, facing > 0.0);
    // Mostly ambient with a soft headlight, like SketchUp's default shading.
    let shade = 0.78 + 0.22 * abs(facing);
    return vec4<f32>(color.rgb * shade, color.a);
}
