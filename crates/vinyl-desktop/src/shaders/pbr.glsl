#version 300 es
precision highp float;
precision highp sampler2DShadow;
uniform vec3 uEye, uSunDirection, uSunRadiance, uSH[9];
uniform float uExposure, uEnvironmentRotation, uSpecularLevels;
uniform sampler2D uEnvironment,uPrefilter,uBRDF;
uniform sampler2DShadow uShadow;uniform mat4 uLVP;uniform vec4 uLamp;
const float PI=3.14159265359;
float hash(vec2 p){p=fract(p*vec2(123.34,456.21));p+=dot(p,p+45.32);return fract(p.x*p.y);}
vec3 rotateEnvironment(vec3 d){float a=uEnvironmentRotation*2.*PI,c=cos(a),s=sin(a);return vec3(c*d.x-s*d.y,s*d.x+c*d.y,d.z);}
vec2 environmentUV(vec3 d){d=rotateEnvironment(normalize(d));return vec2(atan(d.y,d.x)/(2.*PI)+.5,acos(clamp(d.z,-1.,1.))/PI);}
vec3 env(vec3 r,float rough){return textureLod(uPrefilter,environmentUV(r),clamp(rough,0.,1.)*(uSpecularLevels-1.)).rgb;}
vec3 irradiance(vec3 n){
 n=rotateEnvironment(n);float x=n.x,y=n.y,z=n.z;
 return max(vec3(0.),uSH[0]*.282094792+uSH[1]*(.488602512*y)+uSH[2]*(.488602512*z)+uSH[3]*(.488602512*x)
 +uSH[4]*(1.092548431*x*y)+uSH[5]*(1.092548431*y*z)+uSH[6]*(.315391565*(3.*z*z-1.))
 +uSH[7]*(1.092548431*x*z)+uSH[8]*(.546274215*(x*x-y*y)));
}
float shadow(vec3 P,vec3 N){
 vec4 q=uLVP*vec4(P+N*.0008,1.);vec3 s=q.xyz/q.w*.5+.5;
 if(any(lessThan(s,vec3(0.)))||any(greaterThan(s,vec3(1.))))return 1.;
 float a=0.;for(int x=-1;x<=1;x++)for(int y=-1;y<=1;y++)a+=texture(uShadow,vec3(s.xy+vec2(float(x),float(y))*1.4/2048.,s.z-.0005));return a/9.;
}
vec3 fresnel(float cosine,vec3 F0){return F0+(1.-F0)*pow(1.-clamp(cosine,0.,1.),5.);}
vec3 brdf(vec3 N,vec3 V,vec3 L,vec3 F0,vec3 diff,float rough){
 float nl=max(dot(N,L),0.),nv=max(dot(N,V),.0001);if(nl<=0.)return vec3(0.);
 vec3 H=normalize(L+V);float nh=max(dot(N,H),0.),a=max(.0025,rough*rough),a2=a*a;
 float den=nh*nh*(a2-1.)+1.,D=a2/(PI*den*den);float k=(rough+1.)*(rough+1.)/8.;
 float G=nv/(nv*(1.-k)+k)*nl/(nl*(1.-k)+k);vec3 F=fresnel(max(dot(H,V),0.),F0);
 return ((1.-F)*diff/PI+D*G*F/max(4.*nv*nl,.0001))*nl;
}
vec3 shade(vec3 P,vec3 N,vec3 alb,float rough,float metal,float ao){
 rough=clamp(rough,.045,1.);vec3 V=normalize(uEye-P),F0=mix(vec3(.04),alb,metal),diff=alb*(1.-metal);
 float nv=max(dot(N,V),.0001);vec3 F=F0+(max(vec3(1.-rough),F0)-F0)*pow(1.-nv,5.);
 vec2 lut=texture(uBRDF,vec2(nv,rough)).rg;
 vec3 indirect=(1.-F)*diff*irradiance(N)+env(reflect(-V,N),rough)*(F0*lut.x+lut.y);
 vec3 direct=brdf(N,V,uSunDirection,F0,diff,rough)*uSunRadiance*shadow(P,N);
 vec3 lp=uLamp.xyz-P;float ld=length(lp);
 direct+=brdf(N,V,lp/max(ld,.001),F0,diff,max(rough,.3))*vec3(1.,.10,.04)*uLamp.w*.003/(ld*ld+.0012);
 return max(vec3(0.),direct+indirect*clamp(ao,0.,1.));
}
vec3 tone(vec3 x){
 x=max(x*uExposure,vec3(0.));x=clamp((x*(2.51*x+.03))/(x*(2.43*x+.59)+.14),0.,1.);
 return pow(x,vec3(1./2.2))+(hash(gl_FragCoord.xy)-.5)/255.;
}
