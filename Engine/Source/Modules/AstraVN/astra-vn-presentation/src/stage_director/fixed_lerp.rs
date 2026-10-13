use super::*;

pub(super) fn fixed_lerp(
    start: FixedScalar,
    end: FixedScalar,
    progress: FixedScalar,
) -> Result<FixedScalar, VnError> {
    if !(0..=1_000_000).contains(&progress.millionths) {
        return Err(stage_error(
            "ASTRA_VN_STAGE_TWEEN_RANGE",
            "interpolation progress is outside zero and one",
        ));
    }
    let delta = i128::from(end.millionths) - i128::from(start.millionths);
    let value = i128::from(start.millionths)
        + delta
            .checked_mul(i128::from(progress.millionths))
            .ok_or_else(|| stage_error("ASTRA_VN_STAGE_FIXED_RANGE", "interpolation overflowed"))?
            / 1_000_000;
    Ok(FixedScalar {
        millionths: i64::try_from(value).map_err(|_| {
            stage_error(
                "ASTRA_VN_STAGE_FIXED_RANGE",
                "interpolation exceeds the product range",
            )
        })?,
    })
}

pub(super) fn stage_error(code: &str, message: &str) -> VnError {
    VnError::Diagnostic(Diagnostic::blocking(code, message))
}
