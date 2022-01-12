#version 140

in vec3 position;
in vec2 tex_coords;
in vec2 lightmap_coords;
out vec2 v_tex_coords;
out vec2 v_lightmap_coords;

uniform mat4 matrix;

void main() {
    v_tex_coords = tex_coords;
    v_lightmap_coords = lightmap_coords;
    gl_Position = matrix * vec4(position, 1.0);
}