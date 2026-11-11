#version 300 es
precision highp float;
layout(location=0) in vec3 aP;out vec2 vNDC;
void main(){vNDC=aP.xy;gl_Position=vec4(aP.xy,1.,1.);}
