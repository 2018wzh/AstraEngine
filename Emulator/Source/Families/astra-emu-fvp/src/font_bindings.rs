use astra_emu_family_api::{FamilyError, FamilyResult};
use fontdb::{Database, Family, Query, Style, Weight};
use rfvp::subsystem::resources::text_manager::{SystemFontBindings, SystemFontFace};

const MS_GOTHIC: &str = "MS Gothic";
const MS_MINCHO: &str = "MS Mincho";
const MS_PGOTHIC: &str = "MS PGothic";
const MS_PMINCHO: &str = "MS PMincho";

pub(crate) fn load_system_font_bindings() -> FamilyResult<SystemFontBindings> {
    let mut database = Database::new();
    database.load_system_fonts();

    Ok(SystemFontBindings {
        ms_gothic: find_face(&database, MS_GOTHIC, true)?,
        ms_mincho: find_face(&database, MS_MINCHO, false)?,
        ms_pgothic: find_face(&database, MS_PGOTHIC, false)?,
        ms_pmincho: find_face(&database, MS_PMINCHO, false)?,
    })
}

fn find_face(
    database: &Database,
    family_name: &'static str,
    required: bool,
) -> FamilyResult<Option<SystemFontFace>> {
    let families = [Family::Name(family_name)];
    let id = database.query(&Query {
        families: &families,
        weight: Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: Style::Normal,
    });

    let Some(id) = id else {
        return if required {
            Err(missing_font(family_name))
        } else {
            Ok(None)
        };
    };

    let face = database.face(id).ok_or_else(|| {
        FamilyError::invalid(
            "ASTRA_EMU_FVP_FONT_MISSING",
            "the selected system font face disappeared from the font database",
        )
    })?;
    if !face.families.iter().any(|(name, _)| name == family_name) {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_FONT_MISSING",
            "the selected system font face has an unexpected family name",
        ));
    }

    let data = database.with_face_data(id, |bytes, face_index| (bytes.to_vec(), face_index));
    let Some((bytes, face_index)) = data else {
        return Err(FamilyError::invalid(
            "ASTRA_EMU_FVP_FONT_MISSING",
            "the selected system font face data could not be read",
        ));
    };

    Ok(Some(SystemFontFace {
        family_name: family_name.to_owned(),
        bytes,
        face_index,
    }))
}

fn missing_font(family_name: &str) -> FamilyError {
    let message = match family_name {
        MS_GOTHIC => "required MS Gothic system font face is unavailable",
        MS_MINCHO => "requested MS Mincho system font face is unavailable",
        MS_PGOTHIC => "requested MS PGothic system font face is unavailable",
        MS_PMINCHO => "requested MS PMincho system font face is unavailable",
        _ => "requested system font face is unavailable",
    };
    FamilyError::invalid("ASTRA_EMU_FVP_FONT_MISSING", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfvp::subsystem::resources::text_manager::{
        FontEnumerator, FONTFACE_CURRENT, FONTFACE_MS_GOTHIC, FONTFACE_MS_MINCHO,
    };

    // This is a public-domain test fixture only. Production bindings are loaded
    // from the host font database above and never use this file.
    const TEST_FONT_BYTES: &[u8] =
        include_bytes!("../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf");

    fn face(family_name: &str) -> SystemFontFace {
        SystemFontFace {
            family_name: family_name.to_owned(),
            bytes: TEST_FONT_BYTES.to_vec(),
            face_index: 0,
        }
    }

    #[test]
    fn required_system_font_failure_has_stable_family_error() {
        let database = Database::new();
        let error = find_face(&database, MS_GOTHIC, true).expect_err("empty database must fail");
        assert_eq!(error.code(), "ASTRA_EMU_FVP_FONT_MISSING");
        assert!(error.message.contains("MS Gothic"));
    }

    #[test]
    fn hosted_slots_keep_identity_and_do_not_alias_missing_optional_faces() {
        let bindings = SystemFontBindings {
            ms_gothic: Some(face(MS_GOTHIC)),
            ms_mincho: None,
            ms_pgothic: None,
            ms_pmincho: None,
        };
        let mut fonts = FontEnumerator::from_system_font_bindings(bindings).expect("valid fixture");
        fonts
            .init_fontface()
            .expect("required Gothic face is present");

        assert_eq!(
            fonts.get_font_name(FONTFACE_MS_GOTHIC).as_deref(),
            Some(MS_GOTHIC)
        );
        assert_eq!(
            fonts.get_font_name(FONTFACE_MS_MINCHO).as_deref(),
            Some(MS_MINCHO)
        );
        assert!(fonts.get_font(FONTFACE_MS_GOTHIC).is_ok());
        assert!(fonts.get_font(FONTFACE_MS_MINCHO).is_err());

        fonts.set_system_fontface_id(FONTFACE_CURRENT);
        fonts.set_current_font_name(MS_GOTHIC);
        let direct_gothic = fonts.get_font(FONTFACE_MS_GOTHIC).expect("Gothic face");
        let current_gothic = fonts
            .get_font(FONTFACE_CURRENT)
            .expect("current Gothic face");
        assert_eq!(
            current_gothic.metrics('A', 16.0).advance_width,
            direct_gothic.metrics('A', 16.0).advance_width
        );

        fonts.set_current_font_name("Unsupported face");
        assert!(fonts.get_font(FONTFACE_CURRENT).is_err());
    }

    #[test]
    fn hosted_binding_rejects_a_face_with_the_wrong_slot_name() {
        let bindings = SystemFontBindings {
            ms_gothic: Some(face(MS_MINCHO)),
            ..SystemFontBindings::default()
        };
        let error = match FontEnumerator::from_system_font_bindings(bindings) {
            Ok(_) => panic!("slot family mismatch must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("family mismatch"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_binding_carries_the_installed_ms_gothic_face() {
        let bindings = load_system_font_bindings().expect("Windows FVP requires MS Gothic");
        let gothic = bindings.ms_gothic.expect("required Gothic binding");
        assert_eq!(gothic.family_name, MS_GOTHIC);
        assert!(!gothic.bytes.is_empty());
        assert!(gothic.face_index < 64);
    }
}
