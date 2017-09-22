attribute vec4 a_position;
attribute vec4 a_color;
attribute vec2 a_texCoord0;

uniform mat4 u_projTrans;

varying vec4 v_color;
varying vec2 v_texCoords;
varying vec3 v_pos;



void main() {
	v_pos= a_position.xyz;//describes the fragment position in game space
	v_color = a_color;
	v_texCoords = a_texCoord0;

	//bring into clip space
    gl_Position = u_projTrans * a_position;

	//v_color = vec4(tmp.z);//multiply by 2 and use vertex color because normal vertex color is expected to be at 0.5
	// v_color.rgb += vec3(0.2,0.2,.3)*vec3(max(1.0- a_position.z*2.0/1410.0,0.0));
} 