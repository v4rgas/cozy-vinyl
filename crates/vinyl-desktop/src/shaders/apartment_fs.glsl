// Common image-based PBR lighting is prepended by the Rust shader loader.
in vec3 vP,vN;in vec2 vUV;out vec4 o;
uniform sampler2D uTexture,uNormal,uARM;
vec3 materialNormal(vec3 normal){
 vec3 dp1=dFdx(vP),dp2=dFdy(vP);vec2 uv1=dFdx(vUV),uv2=dFdy(vUV);
 vec3 p2=cross(dp2,normal),p1=cross(normal,dp1);
 vec3 T=p2*uv1.x+p1*uv2.x,B=p2*uv1.y+p1*uv2.y;
 float scale=inversesqrt(max(max(dot(T,T),dot(B,B)),1e-12));
 vec3 map=texture(uNormal,vUV).xyz*2.-1.;return normalize(mat3(T*scale,B*scale,normal)*map);
}
void main(){
 vec3 albedo=texture(uTexture,vUV).rgb;vec3 arm=texture(uARM,vUV).rgb;
 vec3 N=materialNormal(normalize(vN));float ao=arm.r;
 // Local contact affects indirect illumination only; sunlight uses the shared map.
 if(vP.z>-.095){vec2 q=abs(vP.xy-vec2(.0425,0.))-vec2(.2275,.19);float d=length(max(q,0.))+min(max(q.x,q.y),0.);ao*=mix(.65,1.,smoothstep(-.015,.035,d));}
 o=vec4(tone(shade(vP,N,albedo,arm.g,arm.b,ao)),1.);
}
