import bevy_pbr::render::{forward_io::{VertexOutput, FragmentOutput}, pbr_fragment::pbr_input_from_standard_material, pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}, pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT};

// The chunk origin modulo 256 is supplied before f32 conversion. All noise periods divide 256.
fn hash_cell(p: vec3<i32>) -> f32 {
    let k = vec3<u32>(p) & vec3<u32>(4095u);
    var h = (k.x*1597334677u) ^ (k.y*3812015801u) ^ (k.z*2798796415u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h >> 16u) / 65535.0;
}
fn field(p:vec3<f32>, period:i32)->f32{
    let cell=vec3<i32>(floor(p));let mask=vec3<i32>(period-1);
    let t=smoothstep(vec3<f32>(0.0),vec3<f32>(1.0),fract(p));
    let a=mix(hash_cell(cell & mask),hash_cell((cell+vec3<i32>(1,0,0)) & mask),t.x);
    let b=mix(hash_cell((cell+vec3<i32>(0,0,1)) & mask),hash_cell((cell+vec3<i32>(1,0,1)) & mask),t.x);
    return mix(a,b,t.z);
}
@fragment
fn fragment(in: VertexOutput,@builtin(front_facing) front:bool)->FragmentOutput {
    var pbr=pbr_input_from_standard_material(in,front);
    let n=normalize(in.world_normal);
    let w=vec3<f32>(in.uv.x,in.uv_b.x,in.uv.y)-n*0.001;
    let block=u32(round(in.uv_b.y));
    let cell=vec3<i32>(floor(w));
    let coarse=field(w*0.125,32);
    let mottle=hash_cell(cell & vec3<i32>(255));
    let texel=vec3<i32>(floor(w*8.0));
    let grain=hash_cell(texel & vec3<i32>(2047));
    let fade=1.0-smoothstep(0.18,0.8,max(length(dpdx(w)),length(dpdy(w))));
    var tint=vec3<f32>(1.0);
    var rough=0.94;
    if block==1u || block==6u {
        tint=mix(vec3<f32>(0.65,0.87,1.10),vec3<f32>(1.35,1.08,0.69),coarse);
        let fleck=select(-0.09,0.17,grain>0.73);
        tint*=0.91+0.16*mottle+fleck*fade;
        rough=select(0.93,0.73,block==6u);
    } else if block==5u {
        let bark=hash_cell(vec3<i32>(texel.x,cell.y/2,texel.z) & vec3<i32>(2047));
        let fissure=select(0.03,-0.25,bark>0.77);
        tint=vec3<f32>(1.04,0.99,0.94)*(0.87+0.22*mottle+(fissure+(grain-0.5)*0.09)*fade);
        rough=0.83;
    } else {
        let seam=sin(w.y*4.3+1.3*coarse+sin(w.x*0.19635)*0.6);
        tint=mix(vec3<f32>(0.78,0.88,1.03),vec3<f32>(1.17,1.07,0.92),coarse)
            *(0.93+mottle*0.12+(grain-0.5)*0.09*fade+select(0.0,0.045,seam>0.82));
        rough=select(0.97,0.73,block==3u);
    }
    pbr.material.base_color=vec4<f32>(pbr.material.base_color.rgb*tint,1.0);
    pbr.material.perceptual_roughness=rough;
    var color=pbr.material.base_color;
    if (pbr.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT)==0u {color=apply_pbr_lighting(pbr);}
    var out:FragmentOutput;
    out.color=main_pass_post_lighting_processing(pbr,color);
    return out;
}

