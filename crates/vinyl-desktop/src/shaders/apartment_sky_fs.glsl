// Common image-based PBR lighting is prepended by the Rust shader loader.
in vec2 vNDC;out vec4 o;uniform mat4 uInverseVP;
void main(){vec4 w=uInverseVP*vec4(vNDC,1.,1.);vec3 d=normalize(w.xyz/w.w-uEye);o=vec4(tone(texture(uEnvironment,environmentUV(d)).rgb),1.);}
