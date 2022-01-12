#version 140

in vec2 v_tex_coords;
in vec2 v_lightmap_coords;
out vec4 color;

uniform sampler2D tex;
uniform float ambient_light; // 0 for overworld and the end, 0.1 for the nether
uniform float sky_brightness; // computed in ClientWorld.getStarBrightness
uniform float sky_darkness; // 0-1 sky darkening from boss bars
uniform float night_vision_strength; // 0-1 how far night vision is on (also used for underwater visibility)
uniform float gamma; // 0-1 gamma setting

float get_brightness(float light_level) {
    return mix(light_level / (4 - 3 * light_level), 1, ambient_light);
}

void main() {
    float sky_brightness = get_brightness(v_lightmap_coords.x) * sky_brightness;
    float block_brightness = get_brightness(v_lightmap_coords.y) * 1.5;
    vec3 block_color = vec3(
        block_brightness,
        block_brightness * ((block_brightness * 0.6 + 0.4) * 0.6 + 0.4),
        block_brightness * (block_brightness * block_brightness * 0.6 + 0.4)
    );
    if (sky_darkness == -1) {
        block_color = mix(block_color, vec3(0.99, 1.12, 1.0), 0.25);
    } else {
        block_color += block_color * sky_brightness;
        block_color = mix(block_color, vec3(0.75, 0.75, 0.75), 0.04);
        block_color = mix(block_color, block_color * vec3(0.7, 0.6, 0.6), sky_darkness);
    }
    block_color = mix(block_color, block_color / max(max(block_color.r, block_color.g), block_color.b), night_vision_strength);
    vec3 ease_out_quart = vec3(1.0, 1.0, 1.0) - block_color;
    ease_out_quart = vec3(1.0, 1.0, 1.0) - (ease_out_quart * ease_out_quart * ease_out_quart * ease_out_quart);
    block_color = mix(block_color, ease_out_quart, gamma);
    block_color = mix(block_color, vec3(0.75, 0.75, 0.75), 0.04);
    block_color = clamp(block_color, vec3(0.0, 0.0, 0.0), vec3(1.0, 1.0, 1.0));

    color = texture(tex, v_tex_coords) * vec4(block_color, 1.0);
}