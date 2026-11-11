// Common image-based PBR lighting is prepended by the Rust shader loader.
in vec3 vP,vN;in vec2 vQ;out vec4 o;
uniform mat4 uM;uniform sampler2D uLut,uVis,uLabel;
uniform float uRLo,uRHi,uRows,uRevEnd,uW,uDust;uniform vec4 uScr[12];uniform int uNScr;
float lut(float r){float x=clamp((r-uRLo)/(uRHi-uRLo),0.,1.)*2047.;int i=int(floor(x));return mix(texelFetch(uLut,ivec2(i,0),0).r,texelFetch(uLut,ivec2(min(i+1,2047),0),0).r,x-float(i));}
void main(){
 float r=length(vQ),phi=atan(vQ.y,vQ.x),ph=phi/6.28318531;
 vec3 N=normalize(vN),V=normalize(uEye-vP);float px=fwidth(vQ.x)+fwidth(vQ.y);vec3 c;
 if(r<.05){vec3 lab=texture(uLabel,vQ/.1+.5).rgb;c=shade(vP,N,lab*lab,.75,0.,1.);}
 else{
   float revs=lut(r),pitch=2e-4/max(1e-5,abs(lut(r-1e-4)-lut(r+1e-4)));
   float g=revs-ph,n=floor(g+.5),inside=step(.01,revs)*step(revs,uRevEnd-.01)*step(r,.1492);
   float dist=abs(fract(g+.5)-.5)*pitch;float sharp=clamp((uW-dist)/max(px,1e-7)+.5,0.,1.);
   float cover=inside*mix(sharp,min(1.,2.*uW/pitch),smoothstep(.25,.9,px/pitch));
   float vis=texture(uVis,vec2(fract(ph),(n+floor(ph)+.5)/uRows)).r;
   vec3 radial=normalize(mat3(uM)*vec3(cos(phi),sin(phi),0.));
   vec3 wallA=normalize(N+radial*.72),wallB=normalize(N-radial*.72);
   float rough=.075+.12*vis;
   vec3 land=shade(vP,N,vec3(.004),.13,0.,1.);
   vec3 walls=.5*(shade(vP,wallA,vec3(.002),rough,0.,1.)+shade(vP,wallB,vec3(.002),rough,0.,1.));
   // A fine V groove reflects along a radial fan. Its bandwidth follows the cut,
   // while the incoming energy and color come from the actual HDR sun.
   vec3 tangent=normalize(mat3(uM)*vec3(-sin(phi),cos(phi),0.));vec3 H=normalize(uSunDirection+V);
   float sig=.012+.045*vis,th=dot(tangent,H);
   walls+=uSunRadiance*shadow(vP,N)*exp(-th*th/(2.*sig*sig))*.0015/sig*max(dot(N,H),0.);
   c=mix(land,walls,cover);
   for(int i=0;i<uNScr;i++){vec4 s=uScr[i];if(r>s.x&&r<s.y){float d=abs(mod(phi-mix(s.z,s.w,(r-s.x)/(s.y-s.x))+3.14159265,6.28318531)-3.14159265)*r;c+=irradiance(N)*.012*(1.-smoothstep(0.,40e-6+px,d));}}
   vec2 cell=floor(vQ/4e-4);if(hash(cell)<uDust*.0016)c+=irradiance(N)*.018*(1.-smoothstep(0.,6e-5+px,length(vQ/4e-4-cell-.5)*4e-4));
 }
 o=vec4(tone(c),1.);
}
