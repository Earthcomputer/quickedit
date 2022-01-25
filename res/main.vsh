#version 140

in vec3 position;
in vec2 tex_coords;
in vec2 lightmap_coords;
in vec3 color;
out vec2 v_tex_coords;
out vec2 v_lightmap_coords;
out vec3 v_color;

uniform mat4 projection_matrix;
uniform mat4 view_matrix;

void main() {
    v_tex_coords = tex_coords;
    v_lightmap_coords = lightmap_coords;
    v_color = color;
    gl_Position = projection_matrix * view_matrix * vec4(position, 1.0);
}