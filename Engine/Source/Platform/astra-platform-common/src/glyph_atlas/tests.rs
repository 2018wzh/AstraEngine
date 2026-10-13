use super::{
    allocate_pending_atlas_slot, pack_atlas, pack_atlas_at_extent, prepare_upload_pixels,
    release_pending_atlas_slot, updated_retained_texture, vertex_upload_required,
    write_padded_resource, AtlasAllocatorState, AtlasResource, AtlasResourceView,
    ResourceMutationJournal, RetainedTexture, ATLAS_PADDING, ATLAS_SIDE, MAX_ATLAS_UPLOAD_BYTES,
    MAX_ATLAS_WIDTH,
};
use astra_media_core::TextureFrame;
use astra_platform::PlatformErrorCode;
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn atlas_width_tracks_total_area_and_packs_multiple_stage_textures() {
    let rgba8 = vec![0x7f; 800 * 600 * 4];
    let texture = TextureFrame {
        width: 800,
        height: 600,
        rgba8: rgba8.into(),
    };
    let resources = (0..8)
        .map(|index| {
            (
                format!("texture.stage.{index}"),
                AtlasResource::texture(texture.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mutations = ResourceMutationJournal::new();
    let packed = pack_atlas(&AtlasResourceView::new(&resources, &mutations))
        .expect("stage textures must fit the bounded atlas");

    assert_eq!(packed.placements.len(), 8);
    assert_eq!(packed.width, 4096);
    assert_eq!(packed.height, 4096);
}

#[test]
fn rectangular_growth_packs_wide_stage_resources_without_raising_resource_budget() {
    let texture = |width, height| {
        AtlasResource::texture(TextureFrame {
            width,
            height,
            rgba8: Vec::new().into(),
        })
    };
    let mut resources = BTreeMap::from([("texture.stage.large".to_string(), texture(2560, 1840))]);
    for index in 0..20 {
        resources.insert(format!("texture.stage.strip.{index}"), texture(512, 1000));
    }
    let mutations = ResourceMutationJournal::new();
    let view = AtlasResourceView::new(&resources, &mutations);

    assert!(pack_atlas_at_extent(&view, ATLAS_SIDE, ATLAS_SIDE)
        .expect("square packing must be evaluated")
        .is_none());
    let packed = pack_atlas_at_extent(&view, MAX_ATLAS_WIDTH, ATLAS_SIDE)
        .expect("rectangular packing must be evaluated")
        .expect("wide stage resources must fit the bounded rectangular atlas");
    assert_eq!((packed.width, packed.height), (8192, 4096));
}

#[test]
fn resource_view_applies_mutations_without_cloning_the_base_map() {
    let texture = |value| {
        let rgba8 = vec![value; 4];
        AtlasResource::texture(TextureFrame {
            width: 1,
            height: 1,
            rgba8: rgba8.into(),
        })
    };
    let base = [
        ("keep".to_string(), texture(1)),
        ("release".to_string(), texture(2)),
        ("replace".to_string(), texture(3)),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    let mutations = [
        ("add".to_string(), Some(texture(4))),
        ("release".to_string(), None),
        ("replace".to_string(), Some(texture(5))),
    ]
    .into_iter()
    .collect::<ResourceMutationJournal>();
    let view = AtlasResourceView::new(&base, &mutations);

    assert_eq!(view.len(), 3);
    assert!(view.contains_key("keep"));
    assert!(view.contains_key("add"));
    assert!(!view.contains_key("release"));
    assert!(view.get("replace") == mutations["replace"].as_ref());
    assert_eq!(
        view.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        vec!["keep", "add", "replace"]
    );
}

#[test]
fn released_atlas_slot_is_reused_without_repacking_live_resources() {
    let rgba8 = vec![0x7f; 64 * 64 * 4];
    let texture = TextureFrame {
        width: 64,
        height: 64,
        rgba8: rgba8.into(),
    };
    let resources = [(
        "texture.stable".to_string(),
        AtlasResource::texture(texture),
    )]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    let mutations = ResourceMutationJournal::new();
    let packed = pack_atlas(&AtlasResourceView::new(&resources, &mutations)).unwrap();
    let original = packed.placements["texture.stable"];
    let mut allocator = AtlasAllocatorState::new(&packed);
    release_pending_atlas_slot(&mut allocator, original).unwrap();
    let reused = allocate_pending_atlas_slot(&mut allocator, 32, 32).unwrap();
    assert_eq!((reused.x, reused.y), (original.x, original.y));
    let original_area = u64::from(original.width + ATLAS_PADDING * 2)
        * u64::from(original.height + ATLAS_PADDING * 2);
    let reused_area = u64::from(32 + ATLAS_PADDING * 2).pow(2);
    assert_eq!(allocator.freed_area, original_area - reused_area);
    assert_eq!(allocator.added_free_slots.len(), 2);
    release_pending_atlas_slot(&mut allocator, reused).unwrap();
    assert_eq!(allocator.freed_area, original_area);
    assert_eq!(allocator.added_free_slots.len(), 1);
    assert_eq!(
        (
            allocator.added_free_slots[0].width,
            allocator.added_free_slots[0].height
        ),
        (
            original.width + ATLAS_PADDING * 2,
            original.height + ATLAS_PADDING * 2
        )
    );
}

#[test]
fn wider_than_current_atlas_requires_repacking_after_row_wrap() {
    let resources = BTreeMap::new();
    let mutations = ResourceMutationJournal::new();
    let packed = pack_atlas(&AtlasResourceView::new(&resources, &mutations)).unwrap();
    assert_eq!(packed.width, 1024);
    let mut allocator = AtlasAllocatorState::new(&packed);
    assert!(allocate_pending_atlas_slot(&mut allocator, 1280, 720).is_err());

    let resources = BTreeMap::from([(
        "texture.stage".to_owned(),
        AtlasResource::texture(TextureFrame {
            width: 1280,
            height: 720,
            rgba8: Vec::new().into(),
        }),
    )]);
    let repacked = pack_atlas(&AtlasResourceView::new(&resources, &mutations)).unwrap();
    let placement = repacked.placements["texture.stage"];
    assert!(placement.x + placement.width + ATLAS_PADDING <= repacked.width);
    assert!(placement.y + placement.height + ATLAS_PADDING <= repacked.height);
}

#[test]
fn stable_vertex_payload_skips_upload_until_bytes_or_buffer_change() {
    let uploaded = [1_u8, 2, 3, 4];
    assert!(!vertex_upload_required(&uploaded, &uploaded, false));
    assert!(vertex_upload_required(&[1, 2, 3, 5], &uploaded, false));
    assert!(vertex_upload_required(&uploaded, &uploaded, true));
}

#[test]
fn rgba_row_upload_preserves_clamped_padding_pixels() {
    let rgba8 = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let resource = AtlasResource::texture(TextureFrame {
        width: 2,
        height: 2,
        rgba8: rgba8.into(),
    });
    let mut destination = vec![0; 4 * 4 * 4];
    write_padded_resource(&resource, &mut destination, 4, 0, 0).unwrap();
    let pixels = destination
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[0])
        .collect::<Vec<_>>();
    assert_eq!(
        pixels,
        vec![1, 1, 5, 5, 1, 1, 5, 5, 9, 9, 13, 13, 9, 9, 13, 13]
    );
}

#[test]
fn partial_texture_update_retains_base_without_full_payload_copy() {
    let rgba8 = astra_media_core::OwnedPixelBuffer::from_vec(vec![1, 2, 3, 4, 5, 6, 7, 8]);
    let base_pointer = rgba8.as_ptr();
    let retained = RetainedTexture {
        base: Arc::new(TextureFrame {
            width: 2,
            height: 1,
            rgba8,
        }),
        patches: vec![],
        patch_bytes: 0,
    };
    let patch = astra_media_core::OwnedPixelBuffer::from_vec(vec![9, 10, 11, 12]);
    let updated = updated_retained_texture(&retained, 1, 0, 1, 1, patch).unwrap();
    assert_eq!(updated.base.rgba8.as_ptr(), base_pointer);
    assert_eq!(updated.patch_bytes, 4);

    let resource = AtlasResource::Texture(Arc::new(updated));
    let mut destination = vec![0; 4 * 3 * 4];
    write_padded_resource(&resource, &mut destination, 4, 0, 0).unwrap();
    let pixels = destination.as_chunks::<4>().0.iter().collect::<Vec<_>>();
    assert_eq!(pixels[5], &[1, 2, 3, 4]);
    assert_eq!(pixels[6], &[9, 10, 11, 12]);
}

#[test]
fn ordered_partial_updates_retain_each_owned_patch_without_full_payload_copy() {
    let rgba8 =
        astra_media_core::OwnedPixelBuffer::from_vec(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    let base_pointer = rgba8.as_ptr();
    let retained = RetainedTexture {
        base: Arc::new(TextureFrame {
            width: 3,
            height: 1,
            rgba8,
        }),
        patches: vec![],
        patch_bytes: 0,
    };
    let first = astra_media_core::OwnedPixelBuffer::from_vec(vec![13, 14, 15, 16]);
    let first_pointer = first.as_ptr();
    let updated = updated_retained_texture(&retained, 0, 0, 1, 1, first).unwrap();
    let second = astra_media_core::OwnedPixelBuffer::from_vec(vec![17, 18, 19, 20]);
    let second_pointer = second.as_ptr();
    let updated = updated_retained_texture(&updated, 2, 0, 1, 1, second).unwrap();

    assert_eq!(updated.base.rgba8.as_ptr(), base_pointer);
    assert_eq!(updated.patches.len(), 2);
    assert_eq!(updated.patches[0].rgba8.as_ptr(), first_pointer);
    assert_eq!(updated.patches[1].rgba8.as_ptr(), second_pointer);
    assert_eq!(updated.patch_bytes, 8);
}

#[test]
fn reserved_atlas_staging_reuses_storage_across_upload_sizes() {
    let mut pixels = Vec::with_capacity(MAX_ATLAS_UPLOAD_BYTES);
    let storage = pixels.as_ptr();

    prepare_upload_pixels(&mut pixels, 800, 600).unwrap();
    assert_eq!(pixels.len(), 800 * 600 * 4);
    assert_eq!(pixels.as_ptr(), storage);

    prepare_upload_pixels(&mut pixels, 1280, 720).unwrap();
    assert_eq!(pixels.len(), 1280 * 720 * 4);
    assert_eq!(pixels.as_ptr(), storage);

    let error = prepare_upload_pixels(&mut pixels, ATLAS_SIDE + 1, ATLAS_SIDE).unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::InvalidState);
}
