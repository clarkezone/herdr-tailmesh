struct SceneUniforms {
    view_projection: mat4x4<f32>,
    group_model: mat4x4<f32>,
    viewport_brightness: vec4<f32>,
    camera_depth: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniforms;

struct ParticleInput {
    @location(0) corner: vec2<f32>,
    @location(1) position_size: vec4<f32>,
    @location(2) color_softness: vec4<f32>,
};

struct ParticleOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color_softness: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn particle_vs(input: ParticleInput) -> ParticleOutput {
    let world = scene.group_model * vec4<f32>(input.position_size.xyz, 1.0);
    var clip = scene.view_projection * world;
    var size_factor = 1.0;
    if scene.camera_depth.z > 0.0 {
        let rear = rear_weight(world.xyz);
        let perspective = clamp(scene.camera_depth.x / max(scene.camera_depth.x-world.z, 0.01), 0.86, 1.16);
        size_factor = perspective * mix(1.0, 0.82, rear);
    }
    let pixel_offset = input.corner
        * input.position_size.w * size_factor
        * scene.viewport_brightness.w
        * 2.0
        / scene.viewport_brightness.xy;
    clip.x += pixel_offset.x * clip.w;
    clip.y += pixel_offset.y * clip.w;

    var output: ParticleOutput;
    output.clip_position = clip;
    output.local = input.corner;
    output.color_softness = input.color_softness;
    output.world = world.xyz;
    return output;
}

fn particle_color(input: ParticleOutput) -> vec4<f32> {
    let distance = length(input.local);
    if distance > 1.0 {
        discard;
    }
    let hard_circle = 1.0 - smoothstep(0.48, 0.82, distance);
    let soft_halo = pow(max(1.0 - distance, 0.0), 1.55);
    let alpha = min(
        hard_circle * input.color_softness.a
            + soft_halo * (1.0 - input.color_softness.a) * 1.45,
        1.0,
    );
    let color = input.color_softness.rgb * scene.viewport_brightness.z * alpha;
    return vec4<f32>(color, alpha);
}

struct LineInput {
    @location(0) corner: vec2<f32>,
    @location(1) start: vec3<f32>,
    @location(2) start_color: vec4<f32>,
    @location(3) end: vec3<f32>,
    @location(4) end_color: vec4<f32>,
};

struct LineOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(linear) side: f32,
};

@vertex
fn line_vs(input: LineInput) -> LineOutput {
    var output: LineOutput;
    let start = scene.group_model * vec4<f32>(input.start, 1.0);
    let end = scene.group_model * vec4<f32>(input.end, 1.0);
    let clip_start = scene.view_projection * start;
    let clip_end = scene.view_projection * end;
    let t = input.corner.x * 0.5 + 0.5;
    let world = mix(start, end, t);
    var clip = mix(clip_start, clip_end, t);
    let direction = (clip_end.xy / clip_end.w - clip_start.xy / clip_start.w)
        * scene.viewport_brightness.xy;
    // Avoid NaNs for zero-length or view-aligned segments.
    let normal = vec2<f32>(-direction.y, direction.x) / max(length(direction), 0.00001);
    // One logical point, plus a physical-pixel antialias fringe on both edges.
    let half_width = (scene.camera_depth.w + 1.0) * 0.5;
    clip.x += normal.x * input.corner.y * half_width * 2.0 / scene.viewport_brightness.x * clip.w;
    clip.y += normal.y * input.corner.y * half_width * 2.0 / scene.viewport_brightness.y * clip.w;
    output.clip_position = clip;
    output.world = world.xyz;
    output.color = mix(input.start_color, input.end_color, t);
    output.side = input.corner.y;
    return output;
}

@fragment
fn line_fs(input: LineOutput) -> @location(0) vec4<f32> {
    return input.color * scene.viewport_brightness.z * line_coverage(input);
}

fn line_coverage(input: LineOutput) -> f32 {
    let half_width = (scene.camera_depth.w + 1.0) * 0.5;
    return clamp(half_width * (1.0 - abs(input.side)), 0.0, 1.0);
}


fn rear_weight(world: vec3<f32>) -> f32 {
    // Camera is on +Z. Feather across the middle of the sphere; no hemisphere
    // pop when rotation carries a cluster through the silhouette/midplane.
    let width = scene.camera_depth.y * 0.32;
    return 1.0 - smoothstep(-width, width, world.z);
}

@fragment
fn particle_fs(input: ParticleOutput) -> @location(0) vec4<f32> {
    return particle_color(input);
}

fn particle_layer(input: ParticleOutput, front: bool) -> vec4<f32> {
    let base = particle_color(input);
    let rear = rear_weight(input.world);
    let halo_strength = mix(1.0, mix(0.45, 0.9, input.color_softness.a), rear);
    let brightness = mix(1.0, 0.42, rear) * halo_strength;
    let weight = select(rear, 1.0-rear, front);
    return vec4<f32>(base.rgb * brightness * weight, base.a * weight);
}
@fragment
fn rear_particle_fs(input: ParticleOutput) -> @location(0) vec4<f32> {
    return particle_layer(input, false);
}
@fragment
fn front_particle_fs(input: ParticleOutput) -> @location(0) vec4<f32> {
    return particle_layer(input, true);
}
fn line_layer(input: LineOutput, front: bool) -> vec4<f32> {
    let rear = rear_weight(input.world);
    let weight = select(rear, 1.0-rear, front);
    return input.color * scene.viewport_brightness.z * mix(1.0, 0.35, rear) * weight * line_coverage(input);
}
@fragment
fn rear_line_fs(input: LineOutput) -> @location(0) vec4<f32> {
    return line_layer(input, false);
}
@fragment
fn front_line_fs(input: LineOutput) -> @location(0) vec4<f32> {
    return line_layer(input, true);
}

struct VeilOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) screen: vec2<f32>,
};
@vertex
fn veil_vs(@location(0) corner: vec2<f32>) -> VeilOutput {
    var output: VeilOutput;
    output.clip_position = vec4<f32>(corner, 0.0, 1.0);
    output.screen = corner;
    return output;
}
@fragment
fn veil_fs(input: VeilOutput) -> @location(0) vec4<f32> {
    let aspect = scene.viewport_brightness.x / max(scene.viewport_brightness.y, 1.0);
    let eye = vec3<f32>(0.0, 0.0, scene.camera_depth.x);
    let ray = normalize(vec3<f32>(input.screen.x*aspect*0.41421356, input.screen.y*0.41421356, -1.0));
    let radius = scene.camera_depth.y;
    let b = dot(eye, ray);
    let discriminant = b*b - dot(eye,eye) + radius*radius;
    if discriminant <= 0.0 { discard; }
    let hit = eye + ray*(-b-sqrt(discriminant));
    let normal = hit / radius;
    let facing = clamp(dot(normal,-ray),0.0,1.0);
    let rim = pow(1.0-facing,3.0);
    let edge = smoothstep(0.0,0.045,facing);
    // Alpha-blended atmosphere lies between rear and front additive layers.
    return vec4<f32>(vec3<f32>(0.015,0.045,0.09)+vec3<f32>(0.04,0.10,0.17)*rim, (0.065+0.14*rim)*edge);
}
