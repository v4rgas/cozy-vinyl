// Common image-based PBR lighting is prepended by the Rust shader loader.
in vec3 vP, vN, vO; out vec4 o;
uniform mat4 uM; uniform vec3 uCol, uEmis; uniform float uRough, uMetal; uniform int uMode; uniform vec2 uPivot;
void main() {
  vec3 N = normalize(vN), alb = uCol; float rough = uRough, metal = uMetal, ao = 1.0;
  float ro = length(vO.xy), px = fwidth(vO.x) + fwidth(vO.y) + fwidth(vO.z);
  if (uMode == 1 || uMode == 3) {                       // lathe-turned: fine concentric brushing
    float b = hash(vec2(floor(ro * 5000.0), 3.0)) - 0.5, fade = 1.0 - smoothstep(1e-4, 6e-4, px);
    if (abs(N.z) > 0.5) { N = normalize(N + mat3(uM) * vec3(vO.xy / max(ro, 1e-4), 0.0) * b * 0.10 * fade); rough += b * 0.10 * fade; }
  }
  if (uMode == 3 && ro > 0.160 && abs(N.z) < 0.4) {     // strobe dots around the platter rim
    float row = clamp(floor((vO.z + 0.0200) / 0.003125), 0.0, 3.0), n = 180.0 - 6.0 * row;
    vec2 cell = vec2((fract(atan(vO.y, vO.x) / 6.2831853 * n) - 0.5) * 1.0336 / n, (fract((vO.z + 0.0200) / 0.003125) - 0.5) * 0.003125);
    float dot_ = 1.0 - smoothstep(0.0008, 0.0010 + px, length(cell));
    alb = mix(vec3(0.02), vec3(0.80, 0.80, 0.82), dot_); rough = mix(0.5, 0.22, dot_);
  }
  if (uMode == 2 && N.z > 0.5) {                        // deck plate: paint texture + contact occlusion
    alb *= 0.88 + 0.24 * hash(floor(vP.xy * 2500.0)) * (1.0 - smoothstep(2e-4, 1e-3, px));
    ao *= mix(0.35, 1.0, smoothstep(0.163, 0.205, length(vP.xy))) * mix(0.45, 1.0, smoothstep(0.028, 0.060, length(vP.xy - uPivot)));
  }
  if (uMode == 5) {                                     // table: occlusion under the plinth
    vec2 q = abs(vP.xy - vec2(0.0425, 0.0)) - vec2(0.2275, 0.19);
    ao *= mix(0.15, 1.0, smoothstep(-0.01, 0.07, length(max(q, 0.0)) + min(max(q.x, q.y), 0.0)));
    alb *= 0.9 + 0.2 * hash(floor(vP.xy * 900.0)) * (1.0 - smoothstep(5e-4, 3e-3, px));
  }
  o = vec4(tone(shade(vP, N, alb, clamp(rough, 0.06, 1.0), metal, ao) + uEmis), 1.0);
}