// Vertex shader
struct Uniforms {
    projection: mat4x4<f32>,
    image_size: vec2<f32>,
    cursor: vec2<f32>,
    alpha: f32,
};

@group(1) @binding(0)
var<uniform> u: Uniforms;

fn getImageSize() -> vec2<f32> {
    return u.image_size;
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(
    model: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.tex_coords = model.tex_coords;
    out.clip_position = u.projection * vec4<f32>(model.position, 1.0);
    return out;
}

// Fragment shader

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;


fn cubicHermite(A: vec3<f32>, B: vec3<f32>, C: vec3<f32>, D: vec3<f32>, t: f32) -> vec3<f32> {
    let t2 = t * t;
    let t3 = t * t * t;
    let a = -A / 2.0 + 3.0 * B / 2.0 - 3.0 * C / 2.0 + D / 2.0;
    let b = A - 2.5 * B + 2.0 * C - D / 2.0;
    let c = -A / 2.0 + C / 2.0;
    let d = B;
    return a * t3 + b * t2 + c * t + d;
}

fn scaleBicubicHermite(P: vec2<f32>) -> vec4<f32> {
    let imageSize = getImageSize();
    let onePixel = 1.0 / imageSize;
    let twoPixels = onePixel * 2.0;

    let P1 = P * imageSize + 0.5;
    let frac = fract(P1);
    let pixel = floor(P1) / imageSize - onePixel / 2.0;

    var C: array<vec3<f32>, 16>;
    var idx: i32 = 0;

    for (var y = -1; y <= 2; y = y + 1) {
        for (var x = -1; x <= 2; x = x + 1) {
            let offset = vec2<f32>(f32(y), f32(x)) * onePixel;
            C[idx] = textureSample(t_diffuse, s_diffuse, pixel + offset).xyz;
            idx = idx + 1;
        }
    }

    var CPX: array<vec3<f32>, 4>;

    for (var i = 0; i < 4; i = i + 1) {
        CPX[i] = cubicHermite(C[i], C[i+4], C[i+8], C[i+12], frac.x);
    }

    return vec4(cubicHermite(CPX[0], CPX[1], CPX[2], CPX[3], frac.y), 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv: vec2<f32> = in.tex_coords;

    var result = scaleBicubicHermite(uv);
    result.w = u.alpha;

    return result;
}
