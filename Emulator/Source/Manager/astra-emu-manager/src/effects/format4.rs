//! Small, strict reader for the Magpie Effect format 4 metadata used by the
//! manager filters.  It deliberately does not implement the Magpie compiler;
//! it only accepts the resource graph and pass declarations required by our
//! own HLSL generator.

use std::collections::BTreeSet;

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Format4Error {
    #[error("format-4 line {line}: {message}")]
    Invalid { line: usize, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectSource {
    pub sort_name: String,
    pub parameters: Vec<Parameter>,
    pub textures: Vec<Texture>,
    pub samplers: Vec<Sampler>,
    pub passes: Vec<Pass>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub ty: ParameterType,
    pub label: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterType {
    Float,
    Int,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub name: String,
    pub format: Option<String>,
    pub width: Option<TextureDimension>,
    pub height: Option<TextureDimension>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureDimension {
    Input,
    InputTimes(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sampler {
    pub name: String,
    pub filter: SamplerFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerFilter {
    Linear,
    Point,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass {
    pub number: u32,
    pub description: String,
    pub inputs: Vec<String>,
    pub output: String,
    pub block_size: u32,
    pub num_threads: u32,
    pub when: Option<String>,
    pub body: String,
}

impl EffectSource {
    pub fn parse(source: &str) -> Result<Self, Format4Error> {
        let lines = source.lines().collect::<Vec<_>>();
        let mut index = 0;
        while index < lines.len() && !lines[index].trim().starts_with("//!MAGPIE EFFECT") {
            index += 1;
        }
        if index == lines.len() {
            return Err(error(1, "missing //!MAGPIE EFFECT"));
        }
        index += 1;
        let mut sort_name = String::new();
        let mut parameters = Vec::new();
        let mut textures = Vec::new();
        let mut samplers = Vec::new();
        let mut passes = Vec::new();
        let mut version_seen = false;
        while index < lines.len() {
            let trimmed = lines[index].trim();
            if trimmed.is_empty() || trimmed.starts_with("//") && !trimmed.starts_with("//!") {
                index += 1;
                continue;
            }
            if trimmed.starts_with("//!PASS ") {
                let (pass, next) = parse_pass(&lines, index)?;
                passes.push(pass);
                index = next;
                continue;
            }
            if trimmed == "//!VERSION 4" {
                if version_seen {
                    return Err(error(index + 1, "duplicate VERSION"));
                }
                version_seen = true;
                index += 1;
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("//!SORT_NAME ") {
                if !sort_name.is_empty() {
                    return Err(error(index + 1, "duplicate SORT_NAME"));
                }
                sort_name = value.trim().to_owned();
                if sort_name.is_empty() {
                    return Err(error(index + 1, "empty SORT_NAME"));
                }
                index += 1;
                continue;
            }
            if trimmed == "//!USE MulAdd" || trimmed == "//!CAPABILITY FP16" {
                index += 1;
                continue;
            }
            if trimmed == "//!PARAMETER" {
                let (parameter, next) = parse_parameter(&lines, index)?;
                parameters.push(parameter);
                index = next;
                continue;
            }
            if trimmed == "//!TEXTURE" {
                let (texture, next) = parse_texture(&lines, index)?;
                textures.push(texture);
                index = next;
                continue;
            }
            if trimmed == "//!SAMPLER" {
                let (sampler, next) = parse_sampler(&lines, index)?;
                samplers.push(sampler);
                index = next;
                continue;
            }
            if trimmed.starts_with("//!") {
                return Err(error(index + 1, format!("unknown directive `{trimmed}`")));
            }
            // Declarations belonging to a parameter/texture/sampler are consumed
            // by their parser. Other code is allowed as common helper HLSL.
            index += 1;
        }
        if !version_seen {
            return Err(error(1, "missing //!VERSION 4"));
        }
        if textures
            .iter()
            .filter(|texture| texture.name == "INPUT")
            .count()
            != 1
            || textures
                .iter()
                .filter(|texture| texture.name == "OUTPUT")
                .count()
                != 1
        {
            return Err(error(
                1,
                "effect must declare exactly one INPUT and OUTPUT texture",
            ));
        }
        let mut names = BTreeSet::new();
        for texture in &textures {
            if !names.insert(texture.name.as_str()) {
                return Err(error(1, format!("duplicate resource `{}`", texture.name)));
            }
        }
        for sampler in &samplers {
            if !names.insert(sampler.name.as_str()) {
                return Err(error(1, format!("duplicate resource `{}`", sampler.name)));
            }
        }
        if passes.is_empty() {
            return Err(error(1, "effect has no passes"));
        }
        let mut available = textures
            .iter()
            .filter(|texture| texture.name == "INPUT" || texture.source.is_some())
            .map(|texture| texture.name.clone())
            .collect::<BTreeSet<_>>();
        for (offset, pass) in passes.iter().enumerate() {
            if pass.number != offset as u32 + 1 {
                return Err(error(1, "passes must be numbered consecutively from 1"));
            }
            if pass.inputs.is_empty() || pass.output.is_empty() || pass.body.trim().is_empty() {
                return Err(error(
                    1,
                    format!("pass {} has incomplete metadata", pass.number),
                ));
            }
            for resource in &pass.inputs {
                if !available.contains(resource) {
                    return Err(error(
                        1,
                        format!("pass references unknown texture `{resource}`"),
                    ));
                }
            }
            if pass.output == "INPUT" || pass.inputs.iter().any(|input| input == &pass.output) {
                return Err(error(
                    1,
                    format!("pass output `{}` is invalid", pass.output),
                ));
            }
            // Format 4 permits pass-local intermediate textures without a
            // //!TEXTURE declaration. They become available to later passes.
            available.insert(pass.output.clone());
        }
        Ok(Self {
            sort_name,
            parameters,
            textures,
            samplers,
            passes,
        })
    }
}

fn parse_parameter(lines: &[&str], start: usize) -> Result<(Parameter, usize), Format4Error> {
    let mut index = start + 1;
    let mut values = [None, None, None, None, None];
    let mut label = None;
    while index < lines.len() && lines[index].trim().starts_with("//!") {
        let line = lines[index].trim();
        let (slot, value) = if let Some(v) = line.strip_prefix("//!DEFAULT ") {
            (0, v)
        } else if let Some(v) = line.strip_prefix("//!LABEL ") {
            label = Some(v.trim().to_owned());
            index += 1;
            continue;
        } else if let Some(v) = line.strip_prefix("//!MIN ") {
            (2, v)
        } else if let Some(v) = line.strip_prefix("//!MAX ") {
            (3, v)
        } else if let Some(v) = line.strip_prefix("//!STEP ") {
            (4, v)
        } else {
            return Err(error(
                index + 1,
                format!("unknown parameter directive `{line}`"),
            ));
        };
        if values[slot].is_some() {
            return Err(error(index + 1, "duplicate parameter field"));
        }
        values[slot] = Some(
            value
                .trim()
                .parse::<f32>()
                .map_err(|_| error(index + 1, "parameter value is not a number"))?,
        );
        index += 1;
    }
    let declaration = lines
        .get(index)
        .ok_or_else(|| error(index + 1, "parameter declaration missing"))?
        .trim();
    let mut parts = declaration.trim_end_matches(';').split_whitespace();
    let ty = match parts.next() {
        Some("float") => ParameterType::Float,
        Some("int") => ParameterType::Int,
        _ => return Err(error(index + 1, "parameter type must be float or int")),
    };
    let name = parts
        .next()
        .ok_or_else(|| error(index + 1, "parameter name missing"))?;
    if parts.next().is_some() || name.is_empty() {
        return Err(error(index + 1, "invalid parameter declaration"));
    }
    let mut characters = name.chars();
    if !characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        || characters.any(|character| character != '_' && !character.is_ascii_alphanumeric())
    {
        return Err(error(
            index + 1,
            "parameter name must be an HLSL identifier",
        ));
    }
    let [Some(default), None, Some(min), Some(max), Some(step)] = values else {
        return Err(error(
            start + 1,
            "parameter requires DEFAULT, MIN, MAX and STEP",
        ));
    };
    if !(min <= default && default <= max && min <= max && step > 0.0) {
        return Err(error(start + 1, "parameter range is invalid"));
    }
    Ok((
        Parameter {
            name: name.to_owned(),
            ty,
            label: label.ok_or_else(|| error(start + 1, "parameter LABEL missing"))?,
            default,
            min,
            max,
            step,
        },
        index + 1,
    ))
}

fn parse_texture(lines: &[&str], start: usize) -> Result<(Texture, usize), Format4Error> {
    let mut index = start + 1;
    let mut format = None;
    let mut width = None;
    let mut height = None;
    let mut source = None;
    while index < lines.len() && lines[index].trim().starts_with("//!") {
        let line = lines[index].trim();
        if let Some(value) = line.strip_prefix("//!FORMAT ") {
            if format.replace(value.trim().to_owned()).is_some() {
                return Err(error(index + 1, "duplicate texture FORMAT"));
            }
        } else if let Some(value) = line.strip_prefix("//!WIDTH ") {
            if width.replace(parse_dimension(value, index + 1)?).is_some() {
                return Err(error(index + 1, "duplicate texture WIDTH"));
            }
        } else if let Some(value) = line.strip_prefix("//!HEIGHT ") {
            if height.replace(parse_dimension(value, index + 1)?).is_some() {
                return Err(error(index + 1, "duplicate texture HEIGHT"));
            }
        } else if let Some(value) = line.strip_prefix("//!SOURCE ") {
            if source.replace(value.trim().to_owned()).is_some() {
                return Err(error(index + 1, "duplicate texture SOURCE"));
            }
            if source.as_deref() == Some("") {
                return Err(error(index + 1, "empty texture SOURCE"));
            }
        } else {
            return Err(error(
                index + 1,
                format!("unknown texture directive `{line}`"),
            ));
        }
        index += 1;
    }
    let declaration = lines
        .get(index)
        .ok_or_else(|| error(index + 1, "texture declaration missing"))?
        .trim();
    let mut parts = declaration.trim_end_matches(';').split_whitespace();
    if parts.next() != Some("Texture2D") {
        return Err(error(index + 1, "texture declaration must use Texture2D"));
    }
    let name = parts
        .next()
        .ok_or_else(|| error(index + 1, "texture name missing"))?;
    if parts.next().is_some() {
        return Err(error(index + 1, "invalid texture declaration"));
    }
    Ok((
        Texture {
            name: name.to_owned(),
            format,
            width,
            height,
            source,
        },
        index + 1,
    ))
}

fn parse_dimension(value: &str, line: usize) -> Result<TextureDimension, Format4Error> {
    let normalized = value.split_whitespace().collect::<String>();
    if normalized == "INPUT_WIDTH" || normalized == "INPUT_HEIGHT" {
        return Ok(TextureDimension::Input);
    }
    let Some(factor) = normalized
        .strip_prefix("INPUT_WIDTH*")
        .or_else(|| normalized.strip_prefix("INPUT_HEIGHT*"))
    else {
        return Err(error(line, "unsupported texture dimension expression"));
    };
    let factor = factor
        .parse::<u32>()
        .map_err(|_| error(line, "invalid texture dimension factor"))?;
    if !(1..=4).contains(&factor) {
        return Err(error(line, "texture dimension factor is out of bounds"));
    }
    Ok(TextureDimension::InputTimes(factor))
}

fn parse_sampler(lines: &[&str], start: usize) -> Result<(Sampler, usize), Format4Error> {
    let mut index = start + 1;
    let mut filter = None;
    while index < lines.len() && lines[index].trim().starts_with("//!") {
        let line = lines[index].trim();
        filter = Some(match line.strip_prefix("//!FILTER ") {
            Some("LINEAR") => SamplerFilter::Linear,
            Some("POINT") => SamplerFilter::Point,
            _ => {
                return Err(error(
                    index + 1,
                    format!("unknown sampler directive `{line}`"),
                ))
            }
        });
        index += 1;
    }
    let declaration = lines
        .get(index)
        .ok_or_else(|| error(index + 1, "sampler declaration missing"))?
        .trim();
    let mut parts = declaration.trim_end_matches(';').split_whitespace();
    if parts.next() != Some("SamplerState") {
        return Err(error(
            index + 1,
            "sampler declaration must use SamplerState",
        ));
    }
    let name = parts
        .next()
        .ok_or_else(|| error(index + 1, "sampler name missing"))?;
    if parts.next().is_some() || filter.is_none() {
        return Err(error(index + 1, "invalid sampler declaration"));
    }
    Ok((
        Sampler {
            name: name.to_owned(),
            filter: filter.unwrap_or(SamplerFilter::Point),
        },
        index + 1,
    ))
}

fn parse_pass(lines: &[&str], start: usize) -> Result<(Pass, usize), Format4Error> {
    let number = lines[start]
        .trim()
        .strip_prefix("//!PASS ")
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or_else(|| error(start + 1, "invalid PASS number"))?;
    let mut index = start + 1;
    let mut description = None;
    let mut inputs = None;
    let mut output = None;
    let mut block_size = 8;
    let mut num_threads = 64;
    let mut when = None;
    while index < lines.len() && lines[index].trim().starts_with("//!") {
        let line = lines[index].trim();
        if let Some(v) = line.strip_prefix("//!DESC ") {
            description = Some(v.trim().to_owned());
        } else if let Some(v) = line.strip_prefix("//!IN ") {
            inputs = Some(
                v.split(|character: char| character.is_whitespace() || character == ',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect(),
            );
        } else if let Some(v) = line.strip_prefix("//!OUT ") {
            output = Some(v.trim().to_owned());
        } else if let Some(v) = line.strip_prefix("//!BLOCK_SIZE ") {
            block_size = v
                .parse()
                .map_err(|_| error(index + 1, "invalid BLOCK_SIZE"))?;
        } else if let Some(v) = line.strip_prefix("//!NUM_THREADS ") {
            num_threads = v
                .parse()
                .map_err(|_| error(index + 1, "invalid NUM_THREADS"))?;
        } else if let Some(v) = line.strip_prefix("//!WHEN ") {
            when = Some(v.trim().to_owned());
        } else {
            return Err(error(index + 1, format!("unknown pass directive `{line}`")));
        }
        index += 1;
    }
    let body_start = index;
    while index < lines.len() && !lines[index].trim().starts_with("//!PASS ") {
        index += 1;
    }
    let body = lines[body_start..index].join("\n");
    if block_size == 0 || num_threads == 0 || num_threads > 1024 {
        return Err(error(start + 1, "pass dispatch bounds are invalid"));
    }
    Ok((
        Pass {
            number,
            description: description.ok_or_else(|| error(start + 1, "DESC missing"))?,
            inputs: inputs.ok_or_else(|| error(start + 1, "IN missing"))?,
            output: output.ok_or_else(|| error(start + 1, "OUT missing"))?,
            block_size,
            num_threads,
            when,
            body,
        },
        index,
    ))
}

fn error(line: usize, message: impl Into<String>) -> Format4Error {
    Format4Error::Invalid {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::EffectSource;

    #[test]
    fn parses_builtin_and_anime_metadata() {
        let scale = include_str!("../../../../../Assets/Effects/Builtin/scale.hlsl");
        let anime = include_str!("../../../../../Assets/Effects/Anime4K/restore_cnn_s.hlsl");
        assert_eq!(EffectSource::parse(scale).unwrap().passes.len(), 1);
        let parsed = EffectSource::parse(anime).unwrap();
        assert_eq!(parsed.passes.len(), 4);
        assert_eq!(parsed.passes[2].output, "conv2d_2_tf");
        assert_eq!(
            parsed.textures[2].format.as_deref(),
            Some("R16G16B16A16_FLOAT")
        );
    }

    #[test]
    fn rejects_unknown_directive() {
        let source = "//!MAGPIE EFFECT\n//!VERSION 4\n//!TEXTURE\nTexture2D INPUT;\n//!TEXTURE\nTexture2D OUTPUT;\n//!SAMPLER\n//!FILTER POINT\nSamplerState sam;\n//!PASS 1\n//!DESC x\n//!IN INPUT\n//!OUT OUTPUT\n//!BOGUS x\nfloat4 Eval1(uint2 p) { return 0; }";
        assert!(EffectSource::parse(source).is_err());
    }
}
