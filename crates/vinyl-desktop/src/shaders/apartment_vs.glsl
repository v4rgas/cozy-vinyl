#version 300 es
precision highp float;
layout(location=0) in vec3 aP; layout(location=1) in vec3 aN; layout(location=2) in vec2 aUV;
uniform mat4 uVP;
out vec3 vP,vN;out vec2 vUV;
void main(){vP=aP;vN=aN;vUV=aUV;gl_Position=uVP*vec4(aP,1.0);}
