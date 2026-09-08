use super::format4::{EffectSource, Pass};

/// Wrap one unchanged format-4  body in host bindings and a wgpu entry
/// point. The pass body remains authored by the selected effect.
pub(crate) fn generate_pass_shader(
    effect: &EffectSource,
    pass: &Pass,
    output_is_rgba8: bool,
) -> Result<String, String> {
    if pass.block_size == 0 || pass.num_threads != 64 {
        return Err(format!(
            "ASTRA_EMU_EFFECT_DISPATCH_UNSUPPORTED_PASS_{}",
            pass.number
        ));
    }
    let entry = format!("Pass{}", pass.number);
    let compact_body = pass.body.split_whitespace().collect::<String>();
    if !compact_body.contains(&format!("void{entry}(uint2blockStart,uint3threadId)")) {
        return Err(format!(
            "ASTRA_EMU_EFFECT_PASS_{}_BODY_MISSING",
            pass.number
        ));
    }
    let sample_sampler = effect
        .samplers
        .first()
        .ok_or_else(|| "ASTRA_EMU_EFFECT_SAMPLER_MISSING".to_owned())?;
    let parameter_fields = effect
        .parameters
        .iter()
        .map(|parameter| format!(" float {};", parameter.name))
        .collect::<String>();
    let parameter_aliases = effect
        .parameters
        .iter()
        .map(|parameter| format!("#define {} params.{}\n", parameter.name, parameter.name))
        .collect::<String>();
    let mut source = "#define MF float\n"
            .to_owned()
            + "#define MF1 float1\n#define MF2 float2\n#define MF3 float3\n#define MF4 float4\n"
            + "#define MF1x1 float1x1\n#define MF1x2 float1x2\n#define MF1x3 float1x3\n#define MF1x4 float1x4\n"
            + "#define MF2x1 float2x1\n#define MF2x2 float2x2\n#define MF2x3 float2x3\n#define MF2x4 float2x4\n"
            + "#define MF3x1 float3x1\n#define MF3x2 float3x2\n#define MF3x3 float3x3\n#define MF3x4 float3x4\n"
            + "#define MF4x1 float4x1\n#define MF4x2 float4x2\n#define MF4x3 float4x3\n#define MF4x4 float4x4\n"
            + &format!("struct FilterParams {{ uint2 input_size; uint2 output_size; float2 input_pt; float2 output_pt; float2 scale_ratio; float strength_value; float _padding0;{} }};\n", parameter_fields)
            + "[[vk::binding(0, 0)]] ConstantBuffer<FilterParams> params : register(b0, space0);\n"
            + "uint2 Rmp8x8(uint a) { return uint2(a / 8, a % 8); }\n"
            + "uint2 GetInputSize() { return params.input_size; }\n"
            + "float2 GetInputPt() { return params.input_pt; }\n"
            + "uint2 GetOutputSize() { return params.output_size; }\n"
            + "float2 GetOutputPt() { return params.output_pt; }\n"
            + "float2 GetScale() { return params.scale_ratio; }\n"
            + "float GetStrength() { return params.strength_value; }\n"
            + "MF2 MulAdd(MF2 x, MF2x2 y, MF2 a) { return mul(x, y) + a; }\n"
            + "MF3 MulAdd(MF2 x, MF2x3 y, MF3 a) { return mul(x, y) + a; }\n"
            + "MF4 MulAdd(MF2 x, MF2x4 y, MF4 a) { return mul(x, y) + a; }\n"
            + "MF2 MulAdd(MF3 x, MF3x2 y, MF2 a) { return mul(x, y) + a; }\n"
            + "MF3 MulAdd(MF3 x, MF3x3 y, MF3 a) { return mul(x, y) + a; }\n"
            + "MF4 MulAdd(MF3 x, MF3x4 y, MF4 a) { return mul(x, y) + a; }\n"
            + "MF2 MulAdd(MF4 x, MF4x2 y, MF2 a) { return mul(x, y) + a; }\n"
            + "MF3 MulAdd(MF4 x, MF4x3 y, MF3 a) { return mul(x, y) + a; }\n"
            + "MF4 MulAdd(MF4 x, MF4x4 y, MF4 a) { return mul(x, y) + a; }\n";
    source.push_str(&parameter_aliases);
    let mut binding = 1_u32;
    for input in &pass.inputs {
        source.push_str(&format!(
            "[[vk::binding({binding}, 0)]] Texture2D<float4> {input} : register(t{}, space0);\n",
            binding - 1
        ));
        binding += 1;
    }
    let output_format = if output_is_rgba8 { "rgba8" } else { "rgba16f" };
    source.push_str(&format!(
        "[[vk::binding({binding}, 0)]] [[vk::image_format(\"{output_format}\")]] RWTexture2D<float4> {} : register(u0, space0);\n",
        pass.output
    ));
    binding += 1;
    for (index, sampler) in effect.samplers.iter().enumerate() {
        source.push_str(&format!(
            "[[vk::binding({}, 0)]] SamplerState {} : register(s{}, space0);\n",
            binding + index as u32,
            sampler.name,
            index
        ));
    }
    for input in &pass.inputs {
        source.push_str(&format!(
            "float4 sample_{input}(uint2 p, int2 offset) {{ int2 q = clamp(int2(p) + offset, int2(0, 0), int2(GetInputSize()) - 1); return {input}.SampleLevel({}, (float2(q) + 0.5) * GetInputPt(), 0); }}\n",
            sample_sampler.name
        ));
    }
    source.push_str(&pass.body);
    source.push_str(&format!(
        "\n[numthreads({}, 1, 1)] void main(uint3 thread_id : SV_GroupThreadID, uint3 group_id : SV_GroupID) {{ {entry}(group_id.xy * {block}, thread_id); }}\n",
        pass.num_threads,
        block = pass.block_size
    ));
    Ok(source)
}

pub(crate) fn uniform_size(effect: &EffectSource) -> usize {
    let raw_size = 48 + effect.parameters.len() * 4;
    raw_size.div_ceil(16) * 16
}

#[cfg(test)]
mod tests {
    use super::{generate_pass_shader, uniform_size, EffectSource};

    #[test]
    fn keeps_original_pass_body_and_adds_only_host_entry() {
        let source = include_str!("../../../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl");
        let effect = EffectSource::parse(source).unwrap();
        let generated = generate_pass_shader(&effect, &effect.passes[0], false).unwrap();
        assert!(generated.contains("void Pass1(uint2 blockStart, uint3 threadId)"));
        assert!(generated.contains("Pass1(group_id.xy * 16, thread_id)"));
        assert!(!generated.contains("Eval1"));
    }

    #[test]
    fn binds_arbitrary_named_float_parameters() {
        let source = "//!MAGPIE EFFECT\n//!VERSION 4\n//!SORT_NAME custom\n//!PARAMETER\n//!DEFAULT 0.5\n//!LABEL Threshold\n//!MIN 0.0\n//!MAX 1.0\n//!STEP 0.1\nfloat threshold;\n//!TEXTURE\nTexture2D INPUT;\n//!TEXTURE\nTexture2D OUTPUT;\n//!SAMPLER\n//!FILTER LINEAR\nSamplerState linear_sampler;\n//!PASS 1\n//!DESC custom\n//!IN INPUT\n//!OUT OUTPUT\nvoid Pass1(uint2 blockStart, uint3 threadId) { OUTPUT[blockStart]=float4(threshold,0,0,1); }";
        let effect = EffectSource::parse(source).unwrap();
        let generated = generate_pass_shader(&effect, &effect.passes[0], true).unwrap();
        assert!(generated.contains("float threshold;"));
        assert!(generated.contains("#define threshold params.threshold"));
        assert_eq!(uniform_size(&effect), 64);
    }
}
