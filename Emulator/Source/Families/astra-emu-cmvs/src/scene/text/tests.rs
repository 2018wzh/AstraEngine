use super::*;
use crate::CmvsNativeTextSurface;

fn fixture() -> (NativeTextRenderer, CmvsPs2aVmState) {
    let mut database = fontdb::Database::new();
    database.load_font_data(
        include_bytes!(
            "../../../../../../../Examples/NativeVN/Assets/Fonts/NotoSansJP-Variable.ttf"
        )
        .to_vec(),
    );
    let mut renderer = NativeTextRenderer {
        database: Some(database),
        ..Default::default()
    };
    renderer.bind_font("Noto Sans JP").unwrap();
    let mut vm = CmvsPs2aVmState::new(0);
    vm.native_text.insert(
        3,
        CmvsNativeTextSurface {
            font: "Noto Sans JP".into(),
            capacity_hint: 0,
            rect: [44, 60, 512, 30],
            cursor: [44, 60],
            size: 30,
            character_spacing: 2,
            line_spacing: 5,
            colors: [0xffffff, 0],
            kinsoku: true,
            style: 2,
            color_mode: 0,
            position: [0, 0],
            glyphs: Vec::new(),
        },
    );
    (renderer, vm)
}

#[test]
fn native_proportional_switch_uses_real_advance_and_fixed_mode_keeps_half_em() {
    let (renderer, mut vm) = fixture();
    renderer.append(&mut vm, 3, "WW").unwrap();
    let proportional = vm.native_text[&3].cursor[0];
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.glyphs.clear();
    surface.cursor = [44, 60];
    vm.process_global_words.insert(0x004f_0580, 0);
    renderer.append(&mut vm, 3, "WW").unwrap();
    assert_eq!(vm.native_text[&3].cursor[0], 44 + 2 * 17);
    assert!(proportional > vm.native_text[&3].cursor[0]);
}

#[test]
fn static_controls_and_ruby_preserve_native_append_and_atomic_error() {
    let (renderer, mut vm) = fixture();
    vm.process_global_words.insert(0x004f_0580, 0);
    renderer
        .append(&mut vm, 3, "A\\n{漢x字/かんじ}\\c\\t\\w1\\@B")
        .unwrap();
    let surface = &vm.native_text[&3];
    assert_eq!(
        surface
            .glyphs
            .iter()
            .map(|glyph| glyph.text.as_str())
            .collect::<String>(),
        "A漢字\\B"
    );
    assert_eq!(surface.glyphs[1].position, [44, 95]);
    let previous = surface.clone();
    assert!(renderer.append(&mut vm, 3, "D\\q").is_err());
    assert_eq!(vm.native_text[&3], previous);
    assert!(renderer.append(&mut vm, 3, "{漢/ruby").is_err());
    assert_eq!(vm.native_text[&3], previous);
}

#[test]
fn kinsoku_and_narrow_punctuation_use_native_codes() {
    let (renderer, mut vm) = fixture();
    vm.process_global_words.insert(0x004f_0580, 0);
    let surface = vm.native_text.get_mut(&3).unwrap();
    surface.rect[2] = 62;
    renderer.append(&mut vm, 3, "漢、字").unwrap();
    let glyphs = &vm.native_text[&3].glyphs;
    assert_eq!(glyphs[1].position, [76, 60]);
    assert_eq!(glyphs[2].position, [44, 95]);
}

#[test]
fn sdk_glyphs_project_to_native_meshes_and_release_with_surface() {
    let (mut renderer, mut vm) = fixture();
    renderer.append(&mut vm, 3, "漢W").unwrap();
    vm.native_text.get_mut(&3).unwrap().position = [20, 40];
    let first = renderer.frame(&vm).unwrap();
    assert!(first
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    assert!(first.iter().any(|command| matches!(
        command,
        SceneCommand::Mesh2D {
            material: astra_media_core::MeshMaterial2D::GlyphMask,
            ..
        }
    )));
    assert!(!renderer
        .frame(&vm)
        .unwrap()
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    vm.native_text.clear();
    let clear = renderer.frame(&vm).unwrap();
    assert!(!clear.is_empty());
    assert!(clear
        .iter()
        .all(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
}

#[test]
fn missing_and_unbound_default_fonts_do_not_substitute() {
    let database = fontdb::Database::new();
    assert_eq!(
        fonts::load(&database, "").unwrap_err().code(),
        "ASTRA_EMU_CMVS_DEFAULT_FONT_UNBOUND"
    );
    assert_eq!(
        fonts::load(&database, "Absent Font").unwrap_err().code(),
        "ASTRA_EMU_CMVS_FONT_MISSING"
    );
}
