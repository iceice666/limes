struct Uniforms {
    resolution: vec2<f32>,
    time: f32,
    intensity: f32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

const TAU: f32 = 6.2831853;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0)
    );

    let position = positions[vertex_index];

    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = position * 0.5 + vec2<f32>(0.5, 0.5);

    return output;
}

fn saturate(value: f32) -> f32 {
    return clamp(value, 0.0, 1.0);
}

fn ease_out_cubic(value: f32) -> f32 {
    let t = saturate(value);
    return 1.0 - pow(1.0 - t, 3.0);
}

fn hash21(value: vec2<f32>) -> f32 {
    return fract(sin(dot(value, vec2<f32>(127.1, 311.7))) * 43758.5453123);
}

fn hash22(value: vec2<f32>) -> vec2<f32> {
    return fract(sin(vec2<f32>(
        dot(value, vec2<f32>(269.5, 183.3)),
        dot(value, vec2<f32>(113.5, 271.9))
    )) * 43758.5453123);
}

fn water_space(uv: vec2<f32>, aspect: f32) -> vec2<f32> {
    // The bundled wallpaper is a top-down view of a figure floating on a
    // water-like surface. Keep rings circular in world space instead of using a
    // horizon/ground-plane projection.
    return vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
}

fn water_visibility(uv: vec2<f32>, aspect: f32) -> f32 {
    // Keep the effect strongest on the open-water areas around the character
    // while still letting thin highlights pass over the reflected clothing.
    let body_soft_mask = 1.0 - smoothstep(
        0.38,
        0.86,
        length((uv - vec2<f32>(0.48, 0.52)) / vec2<f32>(0.42, 0.36))
    );
    let right_pool = smoothstep(0.50, 0.96, uv.x);
    let left_pool = 1.0 - smoothstep(0.16, 0.50, uv.x);
    let bottom_pool = smoothstep(0.56, 0.98, uv.y);
    let upper_pool = 1.0 - smoothstep(0.10, 0.40, uv.y);
    let aspect_fade = smoothstep(0.35, 1.20, length(water_space(uv, aspect) / vec2<f32>(1.20, 0.72)));

    return clamp(
        0.68 + right_pool * 0.20 + left_pool * 0.12 + bottom_pool * 0.08 + upper_pool * 0.07 + aspect_fade * 0.05 - body_soft_mask * 0.22,
        0.44,
        1.08
    );
}

fn ring_mask(point: vec2<f32>, center: vec2<f32>, radius: f32, width: f32) -> f32 {
    let delta = point - center;
    let distance = length(delta);

    return 1.0 - smoothstep(0.0, width, abs(distance - radius));
}

fn local_ring_mask(local: vec2<f32>, radius: f32, width: f32) -> f32 {
    return 1.0 - smoothstep(0.0, width, abs(length(local) - radius));
}

fn splash_mask(point: vec2<f32>, center: vec2<f32>, radius: f32) -> f32 {
    return 1.0 - smoothstep(0.0, radius, length(point - center));
}

fn broken_arc(point: vec2<f32>, center: vec2<f32>, seed: f32, life: f32) -> f32 {
    let direction = normalize(point - center + vec2<f32>(0.0001, -0.0002));
    let wave_a = sin((direction.x * 13.0 + direction.y * 17.0) + seed * TAU + life * 4.0);
    let wave_b = sin((direction.x * -19.0 + direction.y * 11.0) + seed * 11.37 - life * 3.4);
    let wave = max(wave_a, wave_b * 0.78);

    return 0.44 + 0.56 * smoothstep(-0.55, 0.96, wave);
}

fn crescent_highlight(point: vec2<f32>, center: vec2<f32>, seed: f32, life: f32) -> f32 {
    let direction = normalize(point - center + vec2<f32>(0.0002, 0.0001));
    let light_dir = normalize(vec2<f32>(-0.58, -0.82));
    let facing = smoothstep(0.08, 0.94, dot(direction, light_dir));
    let twinkle = 0.72 + 0.28 * sin(seed * TAU + life * 8.0 + direction.x * 9.0 - direction.y * 6.0);

    return facing * twinkle;
}

fn surface_shimmer(uv: vec2<f32>, aspect: f32) -> vec2<f32> {
    let point = water_space(uv, aspect);
    let time = uniforms.time;
    let wave_a = sin(dot(point, vec2<f32>(10.0, 6.5)) + time * 0.72);
    let wave_b = sin(dot(point, vec2<f32>(-6.5, 13.5)) - time * 0.58 + wave_a * 0.42);
    let wave_c = sin(dot(point, vec2<f32>(25.0, -18.0)) + time * 1.36 + wave_b * 0.65);
    let fine = wave_a * 0.44 + wave_b * 0.34 + wave_c * 0.22;
    let thread = sin(dot(point, vec2<f32>(48.0, 33.0)) + time * 2.1 + fine * 1.4);

    let highlights = smoothstep(0.58, 0.98, fine) * 0.82 + smoothstep(0.86, 1.0, thread) * 0.20;
    let troughs = smoothstep(0.56, 0.96, -fine) * 0.55;

    return vec2<f32>(saturate(highlights), saturate(troughs));
}

fn impact_ripples(uv: vec2<f32>, aspect: f32) -> vec4<f32> {
    let point = water_space(uv, aspect);
    let grid_scale = 3.85;
    let base_cell = floor(point * grid_scale);

    var rings = 0.0;
    var splashes = 0.0;
    var halos = 0.0;
    var glints = 0.0;

    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let cell = base_cell + vec2<f32>(f32(x), f32(y));
            let random = hash22(cell);
            let center = (cell + random) / grid_scale;
            let center_uv_x = center.x / aspect + 0.5;
            let right_bias = smoothstep(0.52, 0.96, center_uv_x);
            let left_bias = 1.0 - smoothstep(0.18, 0.54, center_uv_x);
            let activity_threshold = 0.70 - right_bias * 0.30 - left_bias * 0.12;
            let is_active = step(activity_threshold, hash21(cell + vec2<f32>(9.2, 4.7)));
            let rate = 0.16 + random.y * 0.24;
            let life = fract(uniforms.time * rate + random.x);
            let grow = ease_out_cubic(life);
            let fade_in = smoothstep(0.02, 0.13, life);
            let fade_out = 1.0 - smoothstep(0.58, 1.0, life);
            let fade = fade_in * fade_out * is_active;
            let radius = 0.014 + grow * (0.21 + random.y * 0.14);
            let width = 0.0045 + life * 0.018;
            let arc = broken_arc(point, center, random.x, life);
            let main_ring = ring_mask(point, center, radius, width) * arc;
            let inner_ring = ring_mask(point, center, max(radius * 0.55 - 0.010, 0.0), width * 0.72) * (1.0 - smoothstep(0.18, 0.58, life));
            let outer_ring = ring_mask(point, center, radius * 1.27 + 0.020, width * 1.35) * broken_arc(point, center, random.y + 0.37, life) * (1.0 - smoothstep(0.24, 0.92, life));
            let crown = ring_mask(point, center, 0.020 + life * 0.034, 0.008 + life * 0.006) * broken_arc(point, center, random.x + 0.61, life) * (1.0 - smoothstep(0.10, 0.36, life));
            let splash = splash_mask(point, center, 0.014 + random.y * 0.012);
            let halo = splash_mask(point, center, radius * 0.92 + 0.060);
            let glint_offset = vec2<f32>(-0.010, -0.015) * (0.65 + random.x);
            let impact_glint = splash_mask(point, center + glint_offset, 0.008 + random.y * 0.004);
            let ring_glint = (main_ring + outer_ring * 0.45) * crescent_highlight(point, center, random.y, life);

            rings = max(rings, (main_ring + inner_ring * 0.44 + outer_ring * 0.30) * fade);
            splashes = max(splashes, (splash * pow(1.0 - life, 2.6) + crown * 0.82) * is_active);
            halos = max(halos, halo * fade * 0.28);
            glints = max(glints, (ring_glint * 0.78 * fade + impact_glint * pow(1.0 - life, 4.4) * is_active));
        }
    }

    return vec4<f32>(saturate(rings), saturate(splashes), saturate(halos), saturate(glints));
}

fn broad_surface_ripples(uv: vec2<f32>, aspect: f32) -> f32 {
    let point = water_space(uv, aspect);
    let grid_scale = 1.35;
    let base_cell = floor(point * grid_scale);

    var ripples = 0.0;

    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let cell = base_cell + vec2<f32>(f32(x), f32(y));
            let random = hash22(cell + vec2<f32>(31.0, 11.0));
            let center = (cell + random) / grid_scale;
            let life = fract(uniforms.time * (0.040 + random.y * 0.040) + random.x);
            let grow = ease_out_cubic(life);
            let radius = 0.10 + grow * (0.50 + random.y * 0.24);
            let width = 0.009 + life * 0.032;
            let fade = smoothstep(0.02, 0.22, life) * (1.0 - smoothstep(0.66, 1.0, life));
            let main = ring_mask(point, center, radius, width) * broken_arc(point, center, random.y, life);
            let echo = ring_mask(point, center, radius * 0.62 + 0.045, width * 0.78) * broken_arc(point, center, random.x + 0.53, life) * (1.0 - smoothstep(0.28, 0.84, life));
            let glancing = crescent_highlight(point, center, random.x, life) * 0.30 + 0.70;

            ripples = max(ripples, (main + echo * 0.42) * fade * glancing);
        }
    }

    return saturate(ripples);
}

fn rain_impacts(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    // Keep the impact grid in screen/world units. Multiplying by aspect twice
    // makes the hit flashes sub-pixel on widescreen displays.
    let sample = vec2<f32>(uv.x * aspect, uv.y);

    var pins = 0.0;
    var rings = 0.0;
    var mist = 0.0;

    for (var layer: i32 = 0; layer < 2; layer = layer + 1) {
        let layer_f = f32(layer);
        let grid_scale = vec2<f32>(34.0 + layer_f * 20.0, 22.0 + layer_f * 14.0);
        let grid = sample * grid_scale;
        let base_cell = floor(grid);

        for (var y: i32 = -1; y <= 1; y = y + 1) {
            for (var x: i32 = -1; x <= 1; x = x + 1) {
                let cell = base_cell + vec2<f32>(f32(x), f32(y));
                let random = hash22(cell + vec2<f32>(17.0 + layer_f * 13.0, 41.0 - layer_f * 9.0));
                let center = vec2<f32>(0.10, 0.10) + random * 0.80;
                let local = (grid - cell - center) * vec2<f32>(grid_scale.y / grid_scale.x, 1.0);
                let distance = length(local);
                let phase = fract(uniforms.time * (3.0 + random.y * 4.1 + layer_f * 0.85) + random.x);
                let present = step(0.43 + layer_f * 0.08, hash21(cell + vec2<f32>(9.2, 4.7) * (layer_f + 1.0)));
                let approach = 1.0 - smoothstep(0.04, 0.30, phase);
                let ring_fade = smoothstep(0.035, 0.15, phase) * (1.0 - smoothstep(0.38, 0.80, phase));
                let pin = 1.0 - smoothstep(0.000, 0.036 + layer_f * 0.008, distance);
                let bead = 1.0 - smoothstep(0.034, 0.120 + layer_f * 0.026, distance);
                let spray = 1.0 - smoothstep(0.075, 0.260, distance);
                let ring_radius = 0.035 + phase * (0.31 + random.x * 0.08);
                let tiny_ring = local_ring_mask(local, ring_radius, 0.024 + phase * 0.024);
                let crown = local_ring_mask(local, 0.070 + phase * 0.060, 0.030) * (1.0 - smoothstep(0.14, 0.42, phase));
                let brightness = 0.90 + layer_f * 0.18;

                pins = max(pins, present * brightness * (pin * approach + bead * approach * 0.38));
                rings = max(rings, present * brightness * (tiny_ring * ring_fade + crown * approach * 0.42));
                mist = max(mist, present * brightness * spray * (approach * 0.22 + ring_fade * 0.18));
            }
        }
    }

    return vec3<f32>(saturate(pins), saturate(rings), saturate(mist));
}

fn tapered_tail(local: vec2<f32>, tail_dir: vec2<f32>, tail_length: f32, width: f32) -> f32 {
    let along = dot(local, tail_dir);
    let travel = saturate(along / max(tail_length, 0.001));
    let closest = local - tail_dir * clamp(along, 0.0, tail_length);
    let across = length(closest);
    let tapered_width = mix(width * 1.10, width * 0.20, travel);
    let axial = smoothstep(-width * 0.80, width * 0.40, along) * (1.0 - smoothstep(tail_length * 0.70, tail_length, along));
    let lateral = 1.0 - smoothstep(tapered_width * 0.45, tapered_width, across);
    let head_weight = 1.0 - travel * 0.58;

    return saturate(axial * lateral * head_weight);
}

fn top_down_raindrops(uv: vec2<f32>, aspect: f32) -> vec4<f32> {
    // Render the drops as bright beads with a deliberately long motion trace.
    // Neighbor-cell sampling lets the tail extend past its source cell instead
    // of being clipped into a short dot.
    let point = vec2<f32>(uv.x * aspect, uv.y);

    var beads = 0.0;
    var blooms = 0.0;
    var glints = 0.0;
    var trails = 0.0;

    for (var layer: i32 = 0; layer < 4; layer = layer + 1) {
        let layer_f = f32(layer);
        let scale = 30.0 + layer_f * 18.0;
        let p = point * scale + vec2<f32>(layer_f * 19.17, layer_f * 37.31);
        let base_cell = floor(p);

        for (var y: i32 = -1; y <= 2; y = y + 1) {
            for (var x: i32 = -1; x <= 1; x = x + 1) {
                let cell = base_cell + vec2<f32>(f32(x), f32(y));
                let random = hash22(cell + vec2<f32>(71.0 + layer_f * 13.0, 53.0 - layer_f * 7.0));
                let center = vec2<f32>(0.10, 0.10) + random * 0.80;
                let local = p - cell - center;
                let phase = fract(uniforms.time * (2.5 + random.y * 3.7 + layer_f * 0.55) + random.x);
                let active_drop = step(0.52 + layer_f * 0.045, hash21(cell + vec2<f32>(11.0, 29.0) * (layer_f + 1.0)));
                let distance = length(local);
                let focus = smoothstep(0.02, 0.24, phase);
                let approach = 1.0 - smoothstep(0.10, 0.42, phase);
                let impact = smoothstep(0.05, 0.14, phase) * (1.0 - smoothstep(0.24, 0.56, phase));
                let drop_radius = 0.114 - focus * 0.055 + layer_f * 0.004;
                let core = 1.0 - smoothstep(drop_radius * 0.26, drop_radius, distance);
                let soft_bloom = 1.0 - smoothstep(drop_radius * 0.70, drop_radius * 2.15, distance);
                let highlight_offset = vec2<f32>(-0.026, -0.030) * (0.70 + random.y * 0.45);
                let highlight = 1.0 - smoothstep(0.0, drop_radius * 0.24, length(local - highlight_offset));
                let impact_ring = 1.0 - smoothstep(0.0, 0.034 + layer_f * 0.004, abs(distance - (0.060 + phase * 0.275)));
                let tail_dir = normalize(vec2<f32>(-0.14 + (random.x - 0.5) * 0.12, -1.0));
                let side_dir = vec2<f32>(-tail_dir.y, tail_dir.x);
                let side_offset = (random.x - 0.5) * 0.070;
                let tail_length = 1.45 + layer_f * 0.38 + random.y * 0.38;
                let tail_width = 0.055 + layer_f * 0.007;
                let tail_phase = (1.0 - smoothstep(0.40, 0.86, phase)) * (0.72 + focus * 0.28);
                let tail_wide = tapered_tail(local - side_dir * side_offset, tail_dir, tail_length, tail_width);
                let tail_core = tapered_tail(local - side_dir * side_offset, tail_dir, tail_length * 0.82, tail_width * 0.36);
                let brightness = 0.70 + layer_f * 0.08;

                beads = max(beads, active_drop * brightness * core * (approach + impact * 0.36));
                blooms = max(blooms, active_drop * brightness * (soft_bloom * approach * 0.36 + impact_ring * impact * 0.40));
                glints = max(glints, active_drop * brightness * (highlight * approach * 0.82 + impact_ring * impact * 0.24));
                trails = max(trails, active_drop * brightness * (tail_wide * 0.52 + tail_core * 0.78) * tail_phase);
            }
        }
    }

    return vec4<f32>(saturate(beads), saturate(blooms), saturate(glints), saturate(trails));
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(input.uv, vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0));
    let aspect = max(uniforms.resolution.x / max(uniforms.resolution.y, 1.0), 0.25);
    let impacts = impact_ripples(uv, aspect);
    let broad = broad_surface_ripples(uv, aspect);
    let rain = rain_impacts(uv, aspect);
    let drops = top_down_raindrops(uv, aspect);
    let shimmer = surface_shimmer(uv, aspect);
    let vignette = 1.0 - smoothstep(0.50, 1.02, distance(uv, vec2<f32>(0.5, 0.50)));
    let center_soften = 1.0 - smoothstep(0.00, 0.34, length(water_space(uv, aspect) / vec2<f32>(1.20, 0.78))) * 0.18;
    let water_mask = water_visibility(uv, aspect);
    let effect_mask = vignette * center_soften * water_mask;

    let right_water_boost = 1.0 + smoothstep(0.56, 0.98, uv.x) * 0.30;
    let ring_color = vec3<f32>(0.62, 0.82, 1.0) * impacts.x * 0.70 * right_water_boost;
    let splash_color = vec3<f32>(0.92, 0.98, 1.0) * impacts.y * 0.80 * right_water_boost;
    let halo_color = vec3<f32>(0.28, 0.48, 0.72) * impacts.z * 1.06 * right_water_boost;
    let glint_color = vec3<f32>(1.0, 1.0, 1.0) * impacts.w * 1.00 * right_water_boost;
    let broad_color = vec3<f32>(0.40, 0.62, 0.88) * broad * 0.40 * right_water_boost;
    let rain_color = vec3<f32>(0.80, 0.92, 1.0) * rain.x * 0.64 + vec3<f32>(0.56, 0.78, 1.0) * rain.y * 0.52 + vec3<f32>(0.36, 0.56, 0.82) * rain.z * 0.24;
    let drop_color = vec3<f32>(0.80, 0.92, 1.0) * drops.x * 0.52 + vec3<f32>(0.52, 0.76, 1.0) * drops.y * 0.32 + vec3<f32>(1.0, 1.0, 1.0) * drops.z * 0.58 + vec3<f32>(0.66, 0.84, 1.0) * drops.w * 0.76;
    let shimmer_color = vec3<f32>(0.36, 0.60, 0.92) * shimmer.x * 0.18 + vec3<f32>(0.004, 0.014, 0.030) * shimmer.y * 0.10;
    let water_tint = vec3<f32>(0.010, 0.022, 0.048) * 0.12;

    let color = clamp(
        (ring_color + splash_color + halo_color + glint_color + broad_color + rain_color + drop_color + shimmer_color + water_tint) * effect_mask,
        vec3<f32>(0.0, 0.0, 0.0),
        vec3<f32>(1.0, 1.0, 1.0)
    );
    let alpha = clamp(
        (impacts.x * 0.28 + impacts.y * 0.22 + impacts.z * 0.12 + impacts.w * 0.18 + broad * 0.14 + rain.x * 0.22 + rain.y * 0.18 + rain.z * 0.10 + drops.x * 0.18 + drops.y * 0.13 + drops.z * 0.18 + drops.w * 0.30 + shimmer.x * 0.045 + shimmer.y * 0.030 + 0.032) * uniforms.intensity * effect_mask,
        0.0,
        0.62
    );

    return vec4<f32>(color, alpha);
}
