use super::*;
use astra_core::Hash256;
use astra_text::PackagedFont;

pub(super) fn load(database: &fontdb::Database, name: &str) -> Result<PackagedFont, CoreError> {
    // Empty names refer to the native process configuration. Do not substitute
    // a platform-dependent generic family until that configuration is bound.
    if name.is_empty() {
        return Err(error("ASTRA_EMU_CMVS_DEFAULT_FONT_UNBOUND"));
    }
    let face = database
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
        })
        .min_by_key(|face| {
            (
                face.style != fontdb::Style::Normal,
                face.weight.0.abs_diff(400),
            )
        })
        .ok_or_else(|| error("ASTRA_EMU_CMVS_FONT_MISSING"))?;
    let (bytes, index) = database
        .with_face_data(face.id, |bytes, index| (bytes.to_vec(), index))
        .ok_or_else(|| error("ASTRA_EMU_CMVS_FONT_READ"))?;
    let hash = Hash256::from_sha256(&bytes);
    Ok(PackagedFont {
        asset_id: format!("asset:/cmvs/font/{hash}/{index}"),
        family: face.families[0].0.clone(),
        face_index: index,
        hash,
        license_id: "system-installed-local-use".into(),
        subset: None,
        coverage: astra_text::font_unicode_coverage(&bytes, index)
            .map_err(|_| error("ASTRA_EMU_CMVS_FONT_COVERAGE"))?,
        targets: vec!["astra-emu-cmvs".into()],
        profiles: vec!["cmvs.native".into()],
        bytes,
    })
}
