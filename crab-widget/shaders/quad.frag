#version 450
layout(constant_id = 0) const int LAYERS = 1;
layout(push_constant) uniform Draw {
    uint layer;
} draw;
layout(set = 0, binding = 0) uniform sampler2DArray frames[LAYERS];
layout(set = 0, binding = 1, std140) uniform Frame {
    vec4 scale_frame;
    vec4 rects[LAYERS];
} frame;
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 color;
void main() {
    color = texture(frames[draw.layer], vec3(v_uv, frame.scale_frame.z));
}
