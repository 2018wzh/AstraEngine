use crate::model::{TranslationError, TranslationProfile, TranslationRequest};

const PROMPT_OVERHEAD_CHARS: usize = 256;

fn select_context(profile: &TranslationProfile, request: &TranslationRequest) -> Vec<String> {
    let current_chars = request.current.rendered().chars().count();
    let mut remaining = (profile.context_chars as usize).saturating_sub(current_chars);
    let mut selected = Vec::new();
    for segment in request
        .recent
        .iter()
        .rev()
        .take(profile.context_segments as usize)
    {
        let rendered = segment.rendered();
        let chars = rendered.chars().count();
        if chars <= remaining {
            remaining -= chars;
            selected.push(rendered);
        }
    }
    selected.reverse();
    selected
}

pub(crate) fn prompt_segment_count(
    profile: &TranslationProfile,
    request: &TranslationRequest,
) -> usize {
    1 + select_context(profile, request).len()
}

pub fn build_prompt(
    profile: &TranslationProfile,
    request: &TranslationRequest,
) -> Result<String, TranslationError> {
    profile.validate()?;
    request.validate()?;

    let current = request.current.rendered();
    let selected = select_context(profile, request);

    let mut prompt = format!(
        "Translate the CURRENT segment into {}. Preserve names, markup, ruby, and line breaks. Return only the translation.\n",
        profile.target_language
    );
    if !profile.custom_prompt.is_empty() {
        prompt.push_str("STYLE:\n");
        prompt.push_str(&profile.custom_prompt);
        prompt.push('\n');
    }
    if !selected.is_empty() {
        prompt.push_str("CONTEXT:\n");
        for segment in selected {
            prompt.push_str(&segment);
            prompt.push('\n');
        }
    }
    prompt.push_str("CURRENT:\n");
    prompt.push_str(&current);
    let limit = profile.context_chars as usize
        + profile.custom_prompt.chars().count()
        + PROMPT_OVERHEAD_CHARS;
    if prompt.chars().count() > limit {
        return Err(TranslationError::ContextLimit);
    }
    Ok(prompt)
}
