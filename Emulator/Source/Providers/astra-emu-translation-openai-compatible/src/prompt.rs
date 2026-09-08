use crate::model::{TranslationError, TranslationProfile, TranslationRequest};
use crate::{MAX_CONTEXT_CHARS, MAX_CONTEXT_SEGMENTS};

pub(crate) fn prompt_segment_count(request: &TranslationRequest) -> usize {
    let current = request.current.rendered();
    let mut remaining = MAX_CONTEXT_CHARS.saturating_sub(current.chars().count());
    let mut count = 1;
    for segment in request.recent.iter().rev().take(MAX_CONTEXT_SEGMENTS) {
        let chars = segment.rendered().chars().count();
        if chars <= remaining {
            remaining -= chars;
            count += 1;
        }
    }
    count
}

pub fn build_prompt(
    profile: &TranslationProfile,
    request: &TranslationRequest,
) -> Result<String, TranslationError> {
    profile.validate()?;
    request.validate()?;

    let current = request.current.rendered();
    let mut selected = Vec::new();
    let mut remaining = MAX_CONTEXT_CHARS.saturating_sub(current.chars().count());
    for segment in request.recent.iter().rev().take(MAX_CONTEXT_SEGMENTS) {
        let rendered = segment.rendered();
        let chars = rendered.chars().count();
        if chars <= remaining {
            remaining -= chars;
            selected.push(rendered);
        }
    }
    selected.reverse();

    let mut prompt = format!(
        "Translate the CURRENT segment into {}. Preserve names, markup, ruby, and line breaks. Return only the translation.\n",
        profile.target_language
    );
    if !selected.is_empty() {
        prompt.push_str("CONTEXT:\n");
        for segment in selected {
            prompt.push_str(&segment);
            prompt.push('\n');
        }
    }
    prompt.push_str("CURRENT:\n");
    prompt.push_str(&current);
    if prompt.chars().count() > MAX_CONTEXT_CHARS + 256 {
        return Err(TranslationError::ContextLimit);
    }
    Ok(prompt)
}
