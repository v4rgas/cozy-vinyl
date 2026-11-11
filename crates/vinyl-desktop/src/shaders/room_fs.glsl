#version 300 es
precision highp float;
precision highp sampler2DShadow;
in vec3 vP, vN;
in vec2 vUV;
uniform sampler2D uTexture;
uniform sampler2DShadow uShadow;
uniform mat4 uLVP;
out vec4 o;

float contactShadow(vec3 normal) {
    // The room already includes baked shadows. Only the nearby tabletop needs
    // the additional shadow of the newly placed turntable.
    vec2 nearby = abs(vP.xy - vec2(.05, 0.0));
    float local = (1.0 - smoothstep(.30, .35, nearby.x))
        * (1.0 - smoothstep(.25, .30, nearby.y))
        * (1.0 - smoothstep(.03, .16, abs(vP.z + .08))) * abs(normal.z);
    if (local < .001) return 1.0;
    vec4 q = uLVP * vec4(vP + normal * .001, 1.0);
    vec3 p = q.xyz / q.w * .5 + .5;
    if (any(lessThan(p, vec3(0.0))) || any(greaterThan(p, vec3(1.0)))) return 1.0;
    vec2 texel = 1.0 / vec2(textureSize(uShadow, 0));
    float visibility = 0.0;
    for (int x = -1; x <= 1; x++)
        for (int y = -1; y <= 1; y++)
            visibility += texture(uShadow, vec3(p.xy + vec2(float(x), float(y)) * texel, p.z - .0007));
    return 1.0 - .20 * local * (1.0 - visibility / 9.0);
}

void main() {
    vec3 color = texture(uTexture, vUV).rgb * contactShadow(normalize(vN));
    // SRGB8_ALPHA8 decodes at sampling; invert that transfer without exposure
    // or tone mapping so the baked appearance survives a round trip.
    color = mix(color * 12.92, 1.055 * pow(color, vec3(1.0 / 2.4)) - .055,
                step(vec3(.0031308), color));
    o = vec4(color, 1.0);
}
