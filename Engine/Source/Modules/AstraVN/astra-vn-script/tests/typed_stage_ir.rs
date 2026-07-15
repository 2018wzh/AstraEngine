use std::collections::BTreeMap;

use astra_vn_script::{
    compile_astra_sources, compile_astra_sources_with_options, AstraSource, CompileAstraOptions,
    CompiledCommand, ExtensionCommandDescriptor, ExtensionFieldContract, ExtensionFieldKind,
    ExtensionValue, PresentationCommand, StageCommand, TimelineCommand, VnTimelineJoinPolicy,
};

fn source(command: &str) -> AstraSource {
    AstraSource::new(
        "typed.astra",
        format!(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    {command}\n"
        ),
    )
}

fn first_presentation(command: &str) -> PresentationCommand {
    let compiled = compile_astra_sources([source(command)]).unwrap();
    let command = &compiled.states["state.start"].scenes[0].commands[0];
    let CompiledCommand::Presentation { command, .. } = command else {
        panic!("expected presentation command")
    };
    command.clone()
}

#[astra_headless_test::test]
fn standard_commands_lower_to_typed_fixed_point_ir() {
    let command = first_presentation(
        "camera target:main x:-12.5 y:4.25 zoom:1.0625 rotation:0 duration:480 preset:slow_push #@id camera.main",
    );
    let PresentationCommand::Stage(StageCommand::Camera {
        x,
        y,
        zoom,
        duration_ms,
        ..
    }) = command
    else {
        panic!("expected typed camera command")
    };
    assert_eq!(x.millionths, -12_500_000);
    assert_eq!(y.millionths, 4_250_000);
    assert_eq!(zoom.millionths, 1_062_500);
    assert_eq!(duration_ms, 480);
}

#[astra_headless_test::test]
fn timeline_requires_real_ordered_keyframes_and_blocking_fence() {
    let command = first_presentation(
        "timeline id:tl.enter target:hero property:opacity keyframes:0=0,120=0.5,300=1 join:block fence:tl.enter.done fallback:flat budget_ms:2 #@id timeline.enter",
    );
    let PresentationCommand::Stage(StageCommand::Timeline(TimelineCommand::Start(spec))) = command
    else {
        panic!("expected typed timeline")
    };
    assert_eq!(spec.join, VnTimelineJoinPolicy::Block);
    assert_eq!(spec.tracks[0].keyframes.len(), 3);
    assert_eq!(spec.budget_us, 2_000);

    for invalid in [
        "timeline id:tl target:hero property:opacity keyframes:0=0 join:block fence:tl.done budget_ms:2",
        "timeline id:tl target:hero property:opacity keyframes:0=0,0=1 join:block fence:tl.done budget_ms:2",
        "timeline id:tl target:hero property:opacity keyframes:0=0,100=1 join:block budget_ms:2",
    ] {
        let error = compile_astra_sources([source(invalid)]).unwrap_err();
        assert!(
            matches!(
                error.code(),
                "ASTRA_VN_STAGE_ATTRIBUTE_INVALID" | "ASTRA_VN_STAGE_ATTRIBUTE_MISSING"
            ),
            "{error:?}"
        );
    }
}

#[astra_headless_test::test]
fn standard_commands_reject_unknown_fields_and_noncanonical_assets() {
    let unknown = compile_astra_sources([source(
        "show id:hero asset:asset:/character/hero layer:characters opacity:1",
    )])
    .unwrap_err();
    assert_eq!(unknown.code(), "ASTRA_VN_STAGE_ATTRIBUTE_UNKNOWN");

    let path = compile_astra_sources([source(
        "show id:hero asset:native-assets/hero.png layer:characters",
    )])
    .unwrap_err();
    assert_eq!(path.code(), "ASTRA_VN_STAGE_ATTRIBUTE_INVALID");

    let removed = compile_astra_sources([source("task id:legacy")]).unwrap_err();
    assert_eq!(removed.code(), "ASTRA_VN_COMMAND_UNBOUND");
}

#[astra_headless_test::test]
fn extension_commands_require_schema_provider_and_typed_field_contracts() {
    let descriptor = ExtensionCommandDescriptor {
        command: "studio_fx".to_string(),
        provider_id: "studio.presentation".to_string(),
        schema: "studio.presentation.fx.v1".to_string(),
        fields: BTreeMap::from([
            (
                "intensity".to_string(),
                ExtensionFieldContract {
                    kind: ExtensionFieldKind::Fixed,
                    required: true,
                },
            ),
            (
                "enabled".to_string(),
                ExtensionFieldContract {
                    kind: ExtensionFieldKind::Boolean,
                    required: true,
                },
            ),
        ]),
    };
    let compiled = compile_astra_sources_with_options(
        [source("studio_fx intensity:1.25 enabled:true #@id fx.1")],
        CompileAstraOptions::default().bind_extension(descriptor.clone()),
    )
    .unwrap();
    let CompiledCommand::Presentation {
        command: PresentationCommand::Extension(extension),
        ..
    } = &compiled.states["state.start"].scenes[0].commands[0]
    else {
        panic!("expected typed extension command")
    };
    assert_eq!(extension.provider_id, "studio.presentation");
    assert_eq!(
        extension.fields["intensity"],
        ExtensionValue::Fixed(astra_vn_script::FixedScalar {
            millionths: 1_250_000
        })
    );

    for invalid in [
        "studio_fx intensity:1.25",
        "studio_fx intensity:fast enabled:true",
        "studio_fx intensity:1.25 enabled:true hidden:yes",
    ] {
        let error = compile_astra_sources_with_options(
            [source(invalid)],
            CompileAstraOptions::default().bind_extension(descriptor.clone()),
        )
        .unwrap_err();
        assert!(matches!(
            error.code(),
            "ASTRA_VN_STAGE_ATTRIBUTE_MISSING"
                | "ASTRA_VN_STAGE_ATTRIBUTE_INVALID"
                | "ASTRA_VN_STAGE_ATTRIBUTE_UNKNOWN"
        ));
    }

    let conflict = compile_astra_sources_with_options(
        [source("studio_fx intensity:1 enabled:true")],
        CompileAstraOptions::default()
            .bind_extension(descriptor.clone())
            .bind_extension(descriptor.clone()),
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "ASTRA_VN_COMMAND_BINDING_CONFLICT");

    let mut standard_override = descriptor;
    standard_override.command = "show".to_string();
    let conflict = compile_astra_sources_with_options(
        [source(
            "show id:hero asset:asset:/character/hero layer:characters",
        )],
        CompileAstraOptions::default().bind_extension(standard_override),
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "ASTRA_VN_COMMAND_BINDING_CONFLICT");
}
