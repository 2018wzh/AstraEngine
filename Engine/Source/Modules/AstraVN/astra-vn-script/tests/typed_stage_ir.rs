use std::collections::BTreeMap;

use astra_vn_script::{
    compile_astra_project, AstraSource, CompileAstraProjectOptions, CompiledCommand,
    ExtensionCommandDescriptor, ExtensionFieldContract, ExtensionFieldKind, ExtensionValue,
    PresentationCommand, StageCommand, StageFitMode, TimelineCommand, VnAudioControlAction,
    VnTimelineJoinPolicy,
};

fn source(command: &str) -> AstraSource {
    AstraSource::story(
        "typed.astra",
        format!(
            "story main #@id story.main\nstate start #@id state.start\n  scene room #@id scene.room\n    {command}\n"
        ),
    )
}

#[astra_headless_test::test]
fn fade_stop_requires_duration_and_completion_fence() {
    let command = first_presentation(
        "audio action:fade_stop target:bgm.main duration:4000 fence:bgm.main.end #@id audio.fade",
    );
    let PresentationCommand::Stage(StageCommand::AudioControl(control)) = command else {
        panic!("expected typed audio control")
    };
    assert_eq!(
        control.action,
        VnAudioControlAction::FadeStop {
            duration_ms: 4_000,
            fence: "bgm.main.end".into(),
        }
    );

    for invalid in [
        "audio action:fade_stop target:bgm.main fence:bgm.main.end",
        "audio action:fade_stop target:bgm.main duration:4000",
    ] {
        assert!(compile_astra_project([source(invalid)], Default::default()).is_err());
    }
}

fn first_presentation(command: &str) -> PresentationCommand {
    let compiled = compile_astra_project([source(command)], Default::default()).unwrap();
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

    let command = first_presentation(
        "show id:sky asset:asset:/stage/sky layer:sky at:center fit:native opacity:1 #@id show.sky",
    );
    let PresentationCommand::Stage(StageCommand::Show { fit, .. }) = command else {
        panic!("expected typed show command")
    };
    assert_eq!(fit, StageFitMode::Native);
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
        let error = compile_astra_project([source(invalid)], Default::default()).unwrap_err();
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
    let unknown = compile_astra_project(
        [source(
            "show id:hero asset:asset:/character/hero layer:characters unknown:1",
        )],
        Default::default(),
    )
    .unwrap_err();
    assert_eq!(unknown.code(), "ASTRA_VN_STAGE_ATTRIBUTE_UNKNOWN");

    let path = compile_astra_project(
        [source(
            "show id:hero asset:native-assets/hero.png layer:characters",
        )],
        Default::default(),
    )
    .unwrap_err();
    assert_eq!(path.code(), "ASTRA_VN_STAGE_ATTRIBUTE_INVALID");

    let removed =
        compile_astra_project([source("task id:legacy")], Default::default()).unwrap_err();
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
    let compiled = compile_astra_project(
        [source("studio_fx intensity:1.25 enabled:true #@id fx.1")],
        CompileAstraProjectOptions::default().bind_extension(descriptor.clone()),
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
        let error = compile_astra_project(
            [source(invalid)],
            CompileAstraProjectOptions::default().bind_extension(descriptor.clone()),
        )
        .unwrap_err();
        assert!(matches!(
            error.code(),
            "ASTRA_VN_STAGE_ATTRIBUTE_MISSING"
                | "ASTRA_VN_STAGE_ATTRIBUTE_INVALID"
                | "ASTRA_VN_STAGE_ATTRIBUTE_UNKNOWN"
        ));
    }

    let conflict = compile_astra_project(
        [source("studio_fx intensity:1 enabled:true")],
        CompileAstraProjectOptions::default()
            .bind_extension(descriptor.clone())
            .bind_extension(descriptor.clone()),
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "ASTRA_VN_COMMAND_BINDING_CONFLICT");

    let mut standard_override = descriptor;
    standard_override.command = "show".to_string();
    let conflict = compile_astra_project(
        [source(
            "show id:hero asset:asset:/character/hero layer:characters",
        )],
        CompileAstraProjectOptions::default().bind_extension(standard_override),
    )
    .unwrap_err();
    assert_eq!(conflict.code(), "ASTRA_VN_COMMAND_BINDING_CONFLICT");
}
