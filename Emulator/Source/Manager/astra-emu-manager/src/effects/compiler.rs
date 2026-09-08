use std::{borrow::Cow, path::Path};

use hassle_rs::{Dxc, HassleError};
use wgpu::naga::{self, back::wgsl::WriterFlags};

pub(crate) fn hlsl_to_wgsl(
    dxc_path: &Path,
    source_name: &str,
    hlsl: &str,
) -> Result<String, String> {
    let dxc = Dxc::new(Some(dxc_path.to_owned())).map_err(format_dxc_error)?;
    let compiler = dxc.create_compiler().map_err(format_dxc_error)?;
    let library = dxc.create_library().map_err(format_dxc_error)?;
    let blob = library
        .create_blob_with_encoding_from_str(hlsl)
        .map_err(format_dxc_error)?;
    let args = [
        "-spirv",
        "-fspv-target-env=vulkan1.1",
        "-fvk-use-dx-layout",
        "-HV",
        "2021",
    ];
    let operation = match compiler.compile(&blob, source_name, "main", "cs_6_6", &args, None, &[]) {
        Ok(operation) => operation,
        Err((operation, status)) => {
            let detail = operation
                .get_error_buffer()
                .ok()
                .and_then(|buffer| library.get_blob_as_string(&buffer.into()).ok())
                .unwrap_or_else(|| "DXC returned no diagnostic text".to_owned());
            return Err(format!("ASTRA_EMU_EFFECT_DXC_COMPILE_{status}: {detail}"));
        }
    };
    let spirv = operation
        .get_result()
        .map_err(format_dxc_error)?
        .to_vec::<u8>();
    if spirv.is_empty() || spirv.len() % 4 != 0 {
        return Err("ASTRA_EMU_EFFECT_DXC_EMPTY_OR_MISALIGNED_SPIRV".into());
    }
    let words = (0..spirv.len() / 4)
        .map(|index| {
            let offset = index * 4;
            u32::from_le_bytes([
                spirv[offset],
                spirv[offset + 1],
                spirv[offset + 2],
                spirv[offset + 3],
            ])
        })
        .collect::<Vec<_>>();
    let options = naga::front::spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: true,
        block_ctx_dump_prefix: None,
    };
    let module = naga::front::spv::Frontend::new(words.into_iter(), &options)
        .parse()
        .map_err(|error| format!("ASTRA_EMU_EFFECT_NAGA_SPV_PARSE: {error}"))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(&module)
        .map_err(|error| format!("ASTRA_EMU_EFFECT_NAGA_VALIDATE: {error}"))?;
    naga::back::wgsl::write_string(&module, &info, WriterFlags::empty())
        .map_err(|error| format!("ASTRA_EMU_EFFECT_NAGA_WGSL: {error}"))
}

pub(crate) fn shader_module(
    device: &wgpu::Device,
    dxc_path: &Path,
    source_name: &str,
    hlsl: &str,
) -> Result<wgpu::ShaderModule, String> {
    let wgsl = hlsl_to_wgsl(dxc_path, source_name, hlsl)?;
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(source_name),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(wgsl)),
    });
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(format!("ASTRA_EMU_EFFECT_WGPU_SHADER_MODULE: {error}"));
    }
    Ok(module)
}

fn format_dxc_error(error: HassleError) -> String {
    format!("ASTRA_EMU_EFFECT_DXC: {error}")
}
