#version 450
layout(constant_id = 0) const int LAYERS = 1;
layout(push_constant) uniform Draw {
    uint layer;
} draw;
layout(set = 0, binding = 1, std140) uniform Frame {
    vec4 scale_frame;
    vec4 rects[LAYERS];
} frame;
layout(location = 0) out vec2 v_uv;
void main() {
    vec2 corner = vec2(gl_VertexIndex & 1, gl_VertexIndex >> 1);
    v_uv = vec2(corner.x, 1.0 - corner.y);
    vec4 rect = frame.rects[draw.layer];
    vec2 canvas = mix(rect.xy, rect.zw, v_uv);
    gl_Position = vec4((canvas * 2.0 - 1.0) * frame.scale_frame.xy, 0.0, 1.0);
}
