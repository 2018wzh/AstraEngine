//!MAGPIE EFFECT
//!VERSION 4
//!SORT_NAME Astra Sharpen
//!PARAMETER
//!DEFAULT 0.35
//!LABEL Strength
//!MIN 0.0
//!MAX 1.0
//!STEP 0.05
float strength;
//!TEXTURE
Texture2D INPUT;
//!TEXTURE
Texture2D OUTPUT;
//!SAMPLER
//!FILTER POINT
SamplerState point_sampler;
//!PASS 1
//!DESC Three tap sharpen
//!IN INPUT
//!OUT OUTPUT
//!BLOCK_SIZE 8
//!NUM_THREADS 64
float4 sample_point(uint2 p, int2 offset) {
    int2 q = clamp(int2(p) + offset, int2(0, 0), int2(GetInputSize()) - 1);
    return INPUT.SampleLevel(point_sampler, (float2(q) + 0.5) * GetInputPt(), 0);
}
void Pass1(uint2 blockStart, uint3 threadId) {
    uint2 p = blockStart + Rmp8x8(threadId.x);
    if (any(p >= GetOutputSize())) return;
    float4 center = sample_point(p, int2(0, 0));
    float4 cross = (sample_point(p, int2(-1, 0)) + sample_point(p, int2(1, 0))
        + sample_point(p, int2(0, -1)) + sample_point(p, int2(0, 1))) * 0.25;
    OUTPUT[p] = saturate(center + (center - cross) * GetStrength());
}
