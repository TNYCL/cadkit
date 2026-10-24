// 2D plan line and arc instances (see cadkit_scene::layout): coverage matches a 1 px
// antialiased CPU stroke, colors are premultiplied gamma. Positions are f32 model units
// relative to the chunk center; the chunk position is computed in f64 on the CPU.

struct Chunk {
    // Chunk center in framebuffer pixels (y down).
    origin: vec2<f32>,
    // Physical pixels per model unit.
    scale: f32,
    // Pixels per point; stroke widths are logical points.
    ppp: f32,
    // Viewport of the draw: left, top, width, height (pixels).
    viewport: vec4<f32>,
};

@group(0) @binding(0) var<uniform> chunk: Chunk;

const KIND_ARC: u32 = 1u;
const START_JOINT: u32 = 4u;
const END_JOINT: u32 = 8u;

struct Instance {
    // Segment start / arc center.
    @location(0) p0: vec2<f32>,
    // Segment end / (radius, 0).
    @location(1) p1: vec2<f32>,
    // Segment: corner bisectors (snorm16x2). Arc: piece boundary angles (f32 bits).
    @location(2) extra: vec2<u32>,
    @location(3) color: vec4<f32>,
    @location(4) width: f32,
    @location(5) flags: u32,
};

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    // Segment: end pixels. Arc: center pixel and (radius in pixels, 0).
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    // Segment: corner bisectors in screen space. Arc: boundary unit vectors in model space.
    @location(3) @interpolate(flat) ja: vec2<f32>,
    @location(4) @interpolate(flat) jb: vec2<f32>,
    // Half width (pixels) and opacity factor.
    @location(5) @interpolate(flat) stroke: vec2<f32>,
    @location(6) @interpolate(flat) flags: u32,
};

fn to_pixels(p: vec2<f32>) -> vec2<f32> {
    return chunk.origin + vec2<f32>(p.x, -p.y) * chunk.scale;
}

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    let v = chunk.viewport;
    return vec4<f32>((p.x - v.x) / v.z * 2.0 - 1.0, 1.0 - (p.y - v.y) / v.w * 2.0, 0.0, 1.0);
}

fn screen_direction(bits: u32) -> vec2<f32> {
    let d = unpack2x16snorm(bits);
    return vec2<f32>(d.x, -d.y);
}

// Antialiasing spans one physical pixel. Strokes thinner than 0.9 px are drawn with a
// half width of one pixel and an opacity equal to their width (a hairline).
fn stroke_of(width: f32) -> vec2<f32> {
    let w = width * chunk.ppp;
    if w <= 0.9 {
        return vec2<f32>(0.5, w);
    }
    return vec2<f32>(w * 0.5, 1.0);
}

// Signed distance from the line along unit `u`; neighbouring pieces use the same argument order.
fn side(u: vec2<f32>, v: vec2<f32>) -> f32 {
    return u.x * v.y - u.y * v.x;
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> Varyings {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index];
    var out: Varyings;
    out.color = instance.color;
    out.flags = instance.flags;
    let stroke = stroke_of(instance.width);
    out.stroke = stroke;
    // Half width, antialiasing and half a pixel of slack.
    let reach = stroke.x + 1.0;
    var p: vec2<f32>;
    if (instance.flags & KIND_ARC) == 0u {
        let a = to_pixels(instance.p0);
        let b = to_pixels(instance.p1);
        let d = b - a;
        let len = length(d);
        var dir = vec2<f32>(1.0, 0.0);
        if len > 1e-6 {
            dir = d / len;
        }
        let n = vec2<f32>(-dir.y, dir.x);
        p = a - dir * reach;
        if corner.x > 0.5 {
            p = b + dir * reach;
        }
        p = p + n * (corner.y * reach);
        out.a = a;
        out.b = b;
        out.ja = screen_direction(instance.extra.x);
        out.jb = screen_direction(instance.extra.y);
    } else {
        let c = to_pixels(instance.p0);
        let r = instance.p1.x * chunk.scale;
        let s = bitcast<f32>(instance.extra.x);
        let e = bitcast<f32>(instance.extra.y);
        let u0 = vec2<f32>(cos(s), sin(s));
        let u1 = vec2<f32>(cos(e), sin(e));
        let e0 = c + vec2<f32>(u0.x, -u0.y) * r;
        let e1 = c + vec2<f32>(u1.x, -u1.y) * r;
        // A piece is at most a quarter turn: a quad along the chord, widened by the sagitta.
        let mid = normalize(vec2<f32>(u0.x + u1.x, -(u0.y + u1.y)));
        let along_axis = vec2<f32>(-mid.y, mid.x);
        let half_chord = length(e1 - e0) * 0.5 + reach;
        let sagitta = r * (1.0 - dot(vec2<f32>(u0.x, -u0.y), mid));
        var along = -half_chord;
        if corner.x > 0.5 {
            along = half_chord;
        }
        var across = -reach;
        if corner.y > 0.0 {
            across = sagitta + reach;
        }
        p = (e0 + e1) * 0.5 + along_axis * along + mid * across;
        out.a = c;
        out.b = vec2<f32>(r, 0.0);
        out.ja = u0;
        out.jb = u1;
    }
    out.position = to_clip(p);
    return out;
}

@fragment
fn fs_main(in: Varyings) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    let reach = in.stroke.x + 0.5;
    var dist: f32;
    var edge = 1.0;
    if (in.flags & KIND_ARC) == 0u {
        let d = in.b - in.a;
        let len = length(d);
        var dir = vec2<f32>(1.0, 0.0);
        if len > 1e-6 {
            dir = d / len;
        }
        let rel = p - in.a;
        let along = dot(rel, dir);
        dist = abs(dot(rel, vec2<f32>(-dir.y, dir.x)));
        // At a corner the bisector gives each pixel to one segment; the band is continuous, the shape a mitre.
        if (in.flags & START_JOINT) != 0u {
            if dot(p - in.a, in.ja) <= 0.0 || along < -reach {
                discard;
            }
        } else {
            edge = edge * clamp(along + 0.5, 0.0, 1.0);
        }
        if (in.flags & END_JOINT) != 0u {
            if dot(p - in.b, in.jb) > 0.0 || along > len + reach {
                discard;
            }
        } else {
            edge = edge * clamp(len - along + 0.5, 0.0, 1.0);
        }
    } else {
        let v = p - in.a;
        let vm = vec2<f32>(v.x, -v.y);
        dist = abs(length(v) - in.b.x);
        let start = side(in.ja, vm);
        let end = side(in.jb, vm);
        if (in.flags & START_JOINT) != 0u {
            if start < 0.0 {
                discard;
            }
        } else {
            edge = edge * clamp(start + 0.5, 0.0, 1.0);
        }
        if (in.flags & END_JOINT) != 0u {
            if end >= 0.0 {
                discard;
            }
        } else {
            edge = edge * clamp(0.5 - end, 0.0, 1.0);
        }
    }
    let coverage = in.stroke.y * clamp(in.stroke.x + 0.5 - dist, 0.0, 1.0) * edge;
    if coverage <= 0.0 {
        discard;
    }
    return in.color * coverage;
}
