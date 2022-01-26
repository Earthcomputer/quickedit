#version 140

in vec3 position;
in vec3 color;
out vec3 v_color;

uniform mat4 projection_matrix;
uniform mat4 view_matrix;

void main() {
    v_color = color;
    gl_Position = projection_matrix * view_matrix * vec4(position, 1.0);
}