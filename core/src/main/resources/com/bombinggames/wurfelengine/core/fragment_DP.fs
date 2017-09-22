//attributes from tint shader
varying vec4 v_color;
varying vec2 v_texCoords;
varying vec3 v_pos;//world position

//our texture samplers
uniform sampler2D u_texture;   //diffuse map
uniform sampler2D u_normals;   //normal map
uniform sampler2D u_depth; 


//values used for shading algorithm...
uniform vec2 u_resBuffer;
uniform vec4 u_ambientColor;
uniform vec3 u_sunNormal;        //light position, normalized
uniform vec4 u_sunColor;   
uniform vec3 u_moonNormal;        //light position, normalized
uniform vec4 u_moonColor;
uniform vec3 u_cameraPos;
uniform vec3 u_playerpos;
uniform vec3 u_localLightPos;
uniform vec3 u_fogColor;  

void main() {
	 
	
    //RGBA of our diffuse color
    vec4 DiffuseColor = texture2D(u_texture, v_texCoords);
//discard if nearer pr equal then previous layer, little delta to prevent rounding erros
	if (DiffuseColor.a <= 0.1 || gl_FragCoord.z-0.00008 <= texture2D(u_depth , gl_FragCoord.xy/u_resBuffer).r){
	//if (DiffuseColor.a <= 0.1){
	 	discard;
	 }
	gl_FragDepth = gl_FragCoord.z;
	
	vec3 ambient = u_ambientColor.rgb*DiffuseColor.rgb;

	vec3 normalColor = texture2D(u_normals, v_texCoords).rgb;
	vec3 N = normalize((normalColor*2.0- 1.0));//-0.058)*1.25 to normalize because normals are not 100% correct
	N.x = -N.x;//x is flipped in texture, so fix this in shaders

	vec3 tint = v_color.rgb*2.0-vec3(1);//scale tint color by two, so that range 0..1 (0.5 default) is now -1..1
	//linear in range from 0.25 to 0.75, then exponential
	tint += exp((max(v_color.rgb, 0.75)-0.75)*2.0)-vec3(1);
	tint += -exp((min(v_color.rgb, 0.25)-0.25)*-2.0)+vec3(1);

	vec3 fog = u_fogColor*(min(exp(max((u_cameraPos.y-v_pos.y)-40.8, 0.0)*0.001),2.5)-1.0);
	
	vec3 l = -normalize(v_pos.xyz-(u_localLightPos.xyz));

	vec3 viewDir = normalize(vec3(0.0,0.5,1.0));

	vec3 sunLight = vec3(u_sunColor) * max(dot(N, u_sunNormal), 0.0);

	DiffuseColor.rgb*= max(sunLight*4.0,1.0);

	vec3 moonLight = vec3(u_moonColor)*2.0 * max(dot(N, u_moonNormal), 0.0);
	DiffuseColor.rgb*= max(moonLight,1.0);	

	//calculate night color
	float nightMix = u_sunNormal.z/-0.2;
	nightMix = clamp(nightMix, 0.0, 1.0);
	//saturation decrease
	vec3 nightcolor = DiffuseColor.rgb-0.6*(DiffuseColor.rgb-vec3(dot(DiffuseColor.rgb, vec3(.222, .707, .071)) ));
	
	//contrast increase
	nightcolor = ((nightcolor.rgb - 0.5) * max(1.0+0.4*u_moonColor.b, 0.0)) + 0.5;
	
	//combine active color with night color
	DiffuseColor.rgb = DiffuseColor.rgb*(1.0-nightMix)+nightcolor*nightMix;
	
	float dist = length(v_pos-u_playerpos);//distance of fragment in game space from playerpos
	vec3 localLight = vec3(0.3,0.3,0.2) * max(dot(N, l), 0.0);
		
	//Calculate the half vector between the light vector and the view vector.
	//This is typically slower than calculating the actual reflection vector
	// due to the normalize function's reciprocal square root
	vec3 H = normalize( l + viewDir );
	float specAngle = max(dot(H, N), 0.0);
 	float specular = pow(specAngle, 16.0);

	vec3 specLocalLight = vec3(0.3,0.3,0.2) * specular;
	localLight += specLocalLight;
	localLight *= 200000.0/(dist*dist);


//vertice color * texture color*(sun, moon and ambient) 
//use vertice color 0.5 as a base level on which light sources are added. Vertice color is handeled as some sort of light source and not emissivity of texture.
		//gl_FragColor = vec4(
	//vec3(normalize (v_pos.xyz-(playerpos.xyz+vec3(10))))/1.0,
//vec3(sunLight*100.0/dist),	
//fog+tint*0.8*( tint*0.2+ sunLight*100000.0/(dist*dist)+ ambientColor.rgb*1.0)*DiffuseColor.rgb,
	//		DiffuseColor.a*v_color.a
	//	);

	//tint changes material color but also is added
	gl_FragColor = v_color*vec4(fog+ DiffuseColor.rgb+ ambient+ localLight,DiffuseColor.a);	
//gl_FragColor = v_color*vec4(80000.0*vec3(texture2D(u_depth , gl_FragCoord.xy/vec2(1920,1080)).r-gl_FragCoord.z),DiffuseColor.a);
	//vec3 depth = texture2D(u_depth, gl_FragCoord.xy/vec2(1920,1080)).rgb;
	//depth = vec3(gl_FragCoord.xy/vec2(1920,1080),1.0);
	//gl_FragColor = vec4(depth,1.0);
}