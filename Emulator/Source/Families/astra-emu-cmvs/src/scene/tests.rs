use super::*;
use crate::CmvsTextureParentState;

fn state() -> CmvsPs2aVmState {
    let mut vm = CmvsPs2aVmState::new(0);
    for (slot, z) in [(0, 2), (1, 1)] {
        vm.texture_parents.insert(
            slot,
            CmvsTextureParentState {
                surface_initialized: true,
                rect_words: Some([0, 0, 2, 2]),
                position_words: Some([0, 0]),
                auxiliary_word: Some(z),
                ..Default::default()
            },
        );
    }
    vm
}

#[test]
fn recovered_geometry_orders_layers_and_rejects_missing_or_overflowing_state() {
    let mut vm = state();
    let slots = [CmvsTextureSlot::Parent(0), CmvsTextureSlot::Parent(1)];
    let geometry = stage_geometry(&vm, slots.into_iter(), slots[0]).unwrap();
    assert_eq!(geometry[0].0, slots[1]);
    vm.texture_parents.get_mut(&0).unwrap().position_words = Some([u32::MAX, 0]);
    assert_eq!(
        stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_TEXTURE_GEOMETRY"
    );
    vm.texture_parents.get_mut(&0).unwrap().position_words = None;
    assert_eq!(
        stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap_err()
            .code(),
        "ASTRA_EMU_CMVS_TEXTURE_PRESENTATION_STATE"
    );
}

#[test]
#[ignore = "requires a hardware GPU"]
fn gpu_composes_recovered_layer_order_and_replaces_texture_sizes() {
    let mut scene = CmvsScene::new().unwrap();
    let vm = state();
    let slots = [CmvsTextureSlot::Parent(0), CmvsTextureSlot::Parent(1)];
    for size in [1, 2, 1] {
        let commands = stage_geometry(&vm, slots.into_iter(), slots[0])
            .unwrap()
            .into_iter()
            .map(|(slot, destination, _)| {
                let color = if slot == slots[0] {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                };
                SceneCommand::Texture {
                    id: slot_id(slot),
                    frame: TextureFrame {
                        width: size,
                        height: size,
                        rgba8: color.repeat((size * size) as usize).into(),
                    },
                    destination,
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }
            })
            .collect();
        let frame = scene.draw(2, 2, commands).unwrap();
        assert!(frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [255, 0, 0, 255]));
    }
    assert_eq!(scene.sequence, 3);
}
