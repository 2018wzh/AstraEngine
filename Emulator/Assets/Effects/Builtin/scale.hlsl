//!MAGPIE EFFECT
//!VERSION 4
//!SORT_NAME Astra Scale
//!PARAMETER
//!DEFAULT 2.0
//!LABEL Scale
//!MIN 1.0
//!MAX 4.0
//!STEP 0.5
float scale;
//!TEXTURE
Texture2D INPUT;
//!TEXTURE
Texture2D OUTPUT;
//!SAMPLER
//!FILTER LINEAR
SamplerState linear_sampler;
//!PASS 1
//!DESC Linear scale
//!IN INPUT
//!OUT OUTPUT
//!BLOCK_SIZE 8
//!NUM_THREADS 64
void Pass1(uint2 blockStart, uint3 threadId) {
    uint2 p = blockStart + Rmp8x8(threadId.x);
    uint2 outputSize = GetOutputSize();
    if (any(p >= outputSize)) return;
    float2 uv = (float2(p) + 0.5) * GetOutputPt();
    OUTPUT[p] = INPUT.SampleLevel(linear_sampler, uv, 0);
}
