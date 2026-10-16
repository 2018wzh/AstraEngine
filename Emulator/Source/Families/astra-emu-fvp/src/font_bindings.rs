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
