#version 300 es
layout(location=0) in vec3 aP;
uniform mat4 uVP, uM; uniform vec3 uWarp[3];   // (harmonic k, amplitude m, phase)
out vec3 vP, vN; out vec2 vQ;
void main() {
  float r = length(aP.xy), phi = atan(aP.y, aP.x), t = max(0.0, (r - 0.0532) / (0.1508 - 0.0532));
  float z = 0.0, dzr = 0.0, dzp = 0.0;
  for (int i = 0; i < 3; i++) {
    float a = uWarp[i].x * phi + uWarp[i].z;
    z += uWarp[i].y * t * t * cos(a); dzr += uWarp[i].y * 2.0 * t / 0.0976 * cos(a); dzp -= uWarp[i].y * t * t * uWarp[i].x * sin(a);
  }
  vec2 er = aP.xy / max(r, 1e-5), ep = vec2(-er.y, er.x);
  vec3 n = normalize(vec3(-dzr * er - dzp / max(r, 1e-3) * ep, 1.0));
  vec4 w = uM * vec4(aP.xy, z, 1.0);
  vP = w.xyz; vN = mat3(uM) * n; vQ = aP.xy; gl_Position = uVP * w;
}