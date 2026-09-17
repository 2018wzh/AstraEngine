use crate::{parse_sc, ScOpcodeCatalog};

use super::*;

#[test]
fn choice_restores_focus_and_commits_the_selected_label() {
    let source = b".select First:left Second:right\r\n.label left\r\n.setglobal branch = 1\r\n.end\r\n.label right\r\n.setglobal branch = 2\r\n.end\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    assert_eq!(vm.step(1).unwrap(), Some(MusicaVmEvent::Choice));
    vm.move_choice(-1).unwrap();
    assert_eq!(vm.choice_display().unwrap().unwrap().1, 1);
    let saved = vm.encode_native_save().unwrap();
    vm.move_choice(1).unwrap();
    vm.restore_native_save(&saved, 2).unwrap();
    vm.commit_choice().unwrap();
    assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
    assert_eq!(vm.state().global_variables.get("branch"), Some(&2));
    let before = vm.state().clone();
    let mut corrupt = MusicaVm::decode_native_save(&saved).unwrap();
    assert!(vm.restore_native_save(&saved, 0).is_err());
    assert_eq!(vm.state(), &before);
    corrupt.choice.as_mut().unwrap().selected_index = Some(9);
    assert!(vm
        .restore_native_save(&postcard::to_allocvec(&corrupt).unwrap(), 3)
        .is_err());
    assert_eq!(vm.state(), &before);
}

#[test]
fn musica_crossfade_reuses_timeline_and_empty_effect_releases_primary_slot() {
    for clearing in ["*", "end", "CrossFade", "CrossFade2", "CrossFade * 320 100"] {
        let source = format!(
            ".effect CrossFade first.png:second.png 32 100\r\n.effect {clearing}\r\n.end\r\n"
        );
        let mut vm = MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source.as_bytes()),
            parse_sc(source.as_bytes(), &ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap();
        assert!(matches!(
            vm.step(1).unwrap(),
            Some(MusicaVmEvent::Effect(_))
        ));
        vm.advance_effect_clock(100_000_000).unwrap();
        assert!(vm.state().effect.is_some());
        assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::EffectCleared));
        assert!(vm.state().effect.is_none());
        assert_eq!(vm.advance_effect_clock(100_000_000).unwrap(), None);
        let save = vm.encode_native_save().unwrap();
        assert!(MusicaVm::decode_native_save(&save)
            .unwrap()
            .effect
            .is_none());
    }
}

#[test]
fn additional_musica_audio_buses_are_independent_and_saved() {
    let source = b".playbgm first.ogg\r\n.playbgm2 second.ogg\r\n.playse4 hit.ogg t\r\n.playbgm2 *\r\n.end\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    for tick in 1..=3 {
        assert!(matches!(
            vm.step(tick).unwrap(),
            Some(MusicaVmEvent::Audio { .. })
        ));
    }
    assert_eq!(vm.state().audio[&0].bus, "bgm");
    assert_eq!(vm.state().audio[&5].bus, "bgm2");
    assert_eq!(vm.state().audio[&6].bus, "se4");
    let save = vm.encode_native_save().unwrap();
    let saved = vm.state().clone();
    assert!(matches!(
        vm.step(4).unwrap(),
        Some(MusicaVmEvent::Audio { .. })
    ));
    assert!(!vm.state().audio[&5].playing);
    assert!(vm.state().audio[&0].playing);
    assert!(vm.state().audio[&6].playing);
    vm.restore_native_save(&save, 4).unwrap();
    assert_eq!(vm.state(), &saved);
}

#[test]
fn deletevar_removes_local_and_global_bindings_without_affecting_other_names() {
    let source = b".set remove = 1\r\n.setglobal remove = 2\r\n.setglobal keep = 3\r\n.deletevar remove\r\n.deletevar absent\r\n.end\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    assert_eq!(vm.step(1).unwrap(), Some(MusicaVmEvent::Terminal));
    assert!(!vm.state().variables.contains_key("remove"));
    assert!(!vm.state().global_variables.contains_key("remove"));
    assert_eq!(vm.state().global_variables.get("keep"), Some(&3));
}

#[test]
fn deterministic_control_flow_wait_and_native_save_round_trip() {
    let source = b".setglobal route = 1\r\n.label loop\r\n.set count = count + 1\r\n.if count < 3 loop\r\n.wait 20\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        Hash256::from_sha256(source),
        script,
        7,
    )
    .unwrap();
    let event = vm.step(1).unwrap().unwrap();
    let MusicaVmEvent::Wait(MusicaWaitState::Time {
        token_id,
        timer_ticks,
        milliseconds,
    }) = event
    else {
        panic!("expected time wait")
    };
    assert_eq!(timer_ticks, 20);
    assert_eq!(milliseconds, 200);
    assert_eq!(vm.state().variables.get("count"), Some(&3));
    let save = vm.encode_native_save().unwrap();
    let state = vm.state().clone();
    vm.resolve_wait(&token_id).unwrap();
    assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
    vm.restore_native_save(&save, 2).unwrap();
    assert_eq!(vm.state(), &state);
}

#[test]
fn unsupported_presentation_command_blocks_without_advancing_silently() {
    let source = b".char 0\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    assert_eq!(
        vm.step(1).unwrap_err(),
        MusicaRuntimeError::UnsupportedOpcode {
            opcode: "char".into(),
            ordinal: 0,
        }
    );
}

#[test]
fn panel_mode_one_uses_the_verified_message_panel_resource() {
    let source = b".panel 1\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Panel { sequence }) = vm.step(1).unwrap() else {
        panic!("expected panel event")
    };
    assert_eq!(sequence, 1);
    assert_eq!(
        vm.state().panel,
        Some(MusicaPanelState {
            mode: 1,
            resource_uri: "musica:/sys/msgPanel.png".into(),
        })
    );
    let save = vm.encode_native_save().unwrap();
    assert_eq!(
        MusicaVm::decode_native_save(&save).unwrap().panel,
        vm.state().panel
    );

    for source in [
        b".panel 2\r\n".as_slice(),
        b".panel 1 -1\r\n".as_slice(),
        b".panel 1 * ../private.png\r\n".as_slice(),
    ] {
        let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
        let mut vm = MusicaVm::new(
            "musica:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1).unwrap_err(), MusicaRuntimeError::Panel);
    }
}

#[test]
fn crossfade2_keeps_the_verified_resource_and_timeline_state() {
    let source = b".effect CrossFade2 first.png:second.png:*:* 320 100\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Effect(frame)) = vm.step(1).unwrap() else {
        panic!("expected effect frame")
    };
    assert_eq!(
        frame.current_resource_uri.as_deref(),
        Some("musica:/bg/first.png")
    );
    assert_eq!(
        frame.next_resource_uri.as_deref(),
        Some("musica:/bg/second.png")
    );
    assert_eq!(frame.alpha_255, 0);
    let effect = vm.state().effect.as_ref().unwrap();
    assert_eq!(effect.alpha_step, 320);
    assert_eq!(effect.interval_ms, 100);
    assert_eq!(effect.resources.len(), 4);
    assert_eq!(effect.visible_current_index, 0);
    assert_eq!(effect.visible_next_index, 1);
    assert_eq!(effect.visible_alpha_255, 0);

    assert_eq!(vm.advance_effect_clock(99_000_000).unwrap(), None);
    let repeated = vm.advance_effect_clock(1_000_000).unwrap().unwrap();
    assert_eq!(
        repeated.current_resource_uri.as_deref(),
        Some("musica:/bg/second.png")
    );
    assert_eq!(repeated.next_resource_uri, None);
    assert_eq!(repeated.alpha_255, 0);
    let effect = vm.state().effect.as_ref().unwrap();
    assert_eq!(effect.visible_current_index, 1);
    assert_eq!(effect.visible_next_index, 2);
    assert_eq!(effect.visible_alpha_255, 0);
    let save = vm.encode_native_save().unwrap();
    let restored = MusicaVm::decode_native_save(&save).unwrap();
    assert_eq!(restored.effect, vm.state().effect);
    assert_eq!(restored.effect_sequence, vm.state().effect_sequence);
}

#[test]
fn crossfade2_rejects_unknown_modes_and_invalid_timeline_values() {
    for source in [
        b".effect UnknownEffect first.png:second.png 320 100\r\n".as_slice(),
        b".effect CrossFade2 first.png:second.png 0 100\r\n".as_slice(),
        b".effect CrossFade2 first.png:second.png 320 0\r\n".as_slice(),
        b".effect CrossFade2 first.png:second.png 320 100 1\r\n".as_slice(),
    ] {
        let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
        let mut vm = MusicaVm::new(
            "musica:/scr/fixture.sc".into(),
            Hash256::from_sha256(source),
            script,
            1,
        )
        .unwrap();
        assert_eq!(vm.step(1).unwrap_err(), MusicaRuntimeError::Effect);
    }
}

#[test]
fn transition_configures_the_following_stage_without_guessing_star_as_a_resource() {
    let source = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/fixture.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Stage(stage)) = vm.step(1).unwrap() else {
        panic!("expected stage event")
    };
    assert_eq!(stage.resource_sequence, vec![None]);
    assert_eq!(stage.reference_position, None);
    assert_eq!(
        stage.background,
        Some(MusicaStageLayer {
            resource_uri: "musica:/bg/BLACK.png".into(),
            x: 0,
            y: 0,
        })
    );
    assert_eq!(stage.transition.mode, 0);
    assert_eq!(stage.transition.resource, None);
    assert_eq!(stage.transition.duration_ticks, 10);
}

#[test]
fn message_uses_the_verified_four_operand_and_joined_tail_contract() {
    let source = b".message 42 voice speaker hello world\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Message {
        presentation_sequence,
        capture_sequence,
        text,
        speaker,
        wait: MusicaWaitState::Input { token_id },
    }) = vm.step(1).unwrap()
    else {
        panic!("expected message input wait")
    };
    assert_eq!(text, "hello world");
    assert_eq!(speaker.as_deref(), Some("speaker"));
    assert_eq!(presentation_sequence, 1);
    assert_eq!(capture_sequence, 2);
    assert_eq!(token_id, "musica.message.1");
    let state = vm.state().message.as_ref().unwrap();
    assert_eq!(state.message_id, 42);
    assert!(state.source.length > 0);
}

#[test]
fn message_preserves_empty_voice_and_speaker_positions() {
    let source = b".message 42   body words\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Message { text, speaker, .. }) = vm.step(1).unwrap() else {
        panic!("expected message input wait")
    };
    assert_eq!(text, "body words");
    assert!(speaker.is_none());
    let state = vm.state().message.as_ref().unwrap();
    assert_eq!(state.message_id, 42);
}

#[test]
fn message_preserves_empty_voice_before_speaker() {
    let source = b".message 42  speaker body\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Message { text, speaker, .. }) = vm.step(1).unwrap() else {
        panic!("expected message input wait")
    };
    assert_eq!(text, "body");
    assert_eq!(speaker.as_deref(), Some("speaker"));
    assert_eq!(vm.state().message.as_ref().unwrap().message_id, 42);
}

#[test]
fn short_message_executes_the_observed_constructor_defaults() {
    let source = b".message 42 incomplete\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Message { text, speaker, .. }) = vm.step(1).unwrap() else {
        panic!("expected empty message update")
    };
    assert!(text.is_empty());
    assert!(speaker.is_none());
    assert_eq!(vm.state().message.as_ref().unwrap().message_id, -1);
}

#[test]
fn audio_resource_suffix_matches_the_observed_volume_pan_contract() {
    assert_eq!(
        parse_audio_resource_spec("theme.ogg[75,-120]").unwrap(),
        MusicaAudioResourceSpec {
            resource: "theme.ogg".into(),
            volume_percent: 75,
            pan_percent: -100,
        }
    );
    assert_eq!(
        parse_audio_resource_spec("theme.ogg[120,25suffix]").unwrap(),
        MusicaAudioResourceSpec {
            resource: "theme.ogg".into(),
            volume_percent: 100,
            pan_percent: 25,
        }
    );
    assert_eq!(
        parse_audio_resource_spec("theme.ogg[broken").unwrap(),
        MusicaAudioResourceSpec {
            resource: "theme.ogg".into(),
            volume_percent: 100,
            pan_percent: 0,
        }
    );
    assert_eq!(
        parse_audio_resource_spec("[50,0]").unwrap_err(),
        MusicaRuntimeError::AudioResource
    );
}

#[test]
fn play_bgm_emits_stable_uri_audio_commands_with_observed_defaults() {
    let source = b".playBGM theme.ogg[50,-25] * * 80\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Audio { commands }) = vm.step(1).unwrap() else {
        panic!("expected BGM commands")
    };
    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0],
        MusicaAudioCommand::LoadResource {
            sequence: 1,
            stream_id: 0,
            resource_uri: "musica:/bgm/theme.ogg".into(),
        }
    );
    assert_eq!(
        commands[1],
        MusicaAudioCommand::Play {
            sequence: 2,
            stream_id: 0,
            volume: 0.4,
            pan: -0.25,
            repeat: true,
            fade_in_ms: 2,
        }
    );
    let state = vm.state().audio.get(&0).unwrap();
    assert_eq!(state.volume_milli, 400);
    assert_eq!(state.pan_milli, -250);
}

#[test]
fn audio_control_token_stops_the_bound_bus_with_fade_out() {
    let source = b".playBGM theme.ogg\r\n.playBGM * * 25\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    assert!(matches!(
        vm.step(1).unwrap(),
        Some(MusicaVmEvent::Audio { .. })
    ));
    let Some(MusicaVmEvent::Audio { commands }) = vm.step(2).unwrap() else {
        panic!("expected BGM stop command")
    };
    assert_eq!(
        commands,
        vec![MusicaAudioCommand::Stop {
            sequence: 3,
            stream_id: 0,
            fade_ms: 25,
        }]
    );
    assert!(!vm.state().audio.get(&0).unwrap().playing);
}

#[test]
fn play_voice_control_token_is_a_bounded_noop_without_active_voice() {
    let source = b".playVoice * false 2 30\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    assert_eq!(
        vm.step(1).unwrap(),
        Some(MusicaVmEvent::Audio {
            commands: Vec::new()
        })
    );
}

#[test]
fn play_se_preserves_repeat_bus_and_resource_metadata() {
    let source = b".playSE click.ogg[75,20] true * 30\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    let Some(MusicaVmEvent::Audio { commands }) = vm.step(1).unwrap() else {
        panic!("expected SE commands")
    };
    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0],
        MusicaAudioCommand::LoadResource {
            sequence: 1,
            stream_id: 1,
            resource_uri: "musica:/se/click.ogg".into(),
        }
    );
    assert_eq!(
        commands[1],
        MusicaAudioCommand::Play {
            sequence: 2,
            stream_id: 1,
            volume: 0.75,
            pan: 0.2,
            repeat: true,
            fade_in_ms: 2,
        }
    );
    let state = vm.state().audio.get(&1).unwrap();
    assert_eq!(state.bus, "se");
    assert!(state.looped);
}

#[test]
fn chain_is_a_bounded_tail_transfer_without_a_return_frame() {
    let source = b".set local = 1\r\n.chain K01.sc\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    assert_eq!(
        vm.step(1).unwrap(),
        Some(MusicaVmEvent::Chain {
            target: "K01.sc".into()
        })
    );

    let next = b".end\r\n";
    vm.replace_script(
        "musica:/scr/K01.sc".into(),
        Hash256::from_sha256(next),
        parse_sc(next, &ScOpcodeCatalog::observed_musica()).unwrap(),
        None,
    )
    .unwrap();
    assert!(vm.state().variables.is_empty());
    assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
}

#[test]
fn chain_rejects_path_escape() {
    let source = b".chain ../outside.sc\r\n";
    assert!(parse_sc(source, &ScOpcodeCatalog::observed_musica()).is_err());
    assert_eq!(
        validate_chain_target("../outside.sc"),
        Err(MusicaRuntimeError::ChainTarget)
    );
}

#[test]
fn chain_label_is_resolved_before_replacing_script_and_survives_save_restore() {
    let source = b".chain next.sc#entry\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/start.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    assert_eq!(
        vm.step(1).unwrap(),
        Some(MusicaVmEvent::Chain {
            target: "next.sc#entry".into()
        })
    );
    let before = vm.state().clone();
    let next = b".setglobal skipped = 1\r\n.label entry\r\n.setglobal reached = 1\r\n.end\r\n";
    let script = parse_sc(next, &ScOpcodeCatalog::observed_musica()).unwrap();
    assert_eq!(
        vm.replace_script(
            "musica:/scr/next.sc".into(),
            Hash256::from_sha256(next),
            script.clone(),
            Some("missing")
        ),
        Err(MusicaRuntimeError::Label)
    );
    assert_eq!(vm.state(), &before);
    vm.replace_script(
        "musica:/scr/next.sc".into(),
        Hash256::from_sha256(next),
        script,
        Some("entry"),
    )
    .unwrap();
    let save = vm.encode_native_save().unwrap();
    assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
    assert!(!vm.state().global_variables.contains_key("skipped"));
    assert_eq!(vm.state().global_variables.get("reached"), Some(&1));
    vm.restore_native_save(&save, 2).unwrap();
    assert_eq!(vm.step(2).unwrap(), Some(MusicaVmEvent::Terminal));
    assert!(!vm.state().global_variables.contains_key("skipped"));
}

#[test]
fn assignment_uses_verified_three_and_five_token_forms() {
    let source = b".set base = 6\r\n.set sum = base + 4\r\n.set bits = sum | 1\r\n.set rem = sum % 4\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    assert_eq!(vm.step(1).unwrap(), Some(MusicaVmEvent::Terminal));
    assert_eq!(vm.state().variables.get("sum"), Some(&10));
    assert_eq!(vm.state().variables.get("bits"), Some(&11));
    assert_eq!(vm.state().variables.get("rem"), Some(&2));

    let unsupported = b".set count += 1\r\n";
    let script = parse_sc(unsupported, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(unsupported),
        script,
        1,
    )
    .unwrap();
    assert_eq!(vm.step(1).unwrap_err(), MusicaRuntimeError::Operand);
}

#[test]
fn panel_modes_replace_clear_and_roundtrip_native_save() {
    let source = b".panel 1 * custom.png\r\n.panel 3\r\n.panel 0\r\n.end\r\n";
    let script = parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap();
    let mut vm = MusicaVm::new(
        "musica:/scr/panel.sc".into(),
        Hash256::from_sha256(source),
        script,
        1,
    )
    .unwrap();
    for (tick, expected) in [
        (
            1,
            Some(MusicaPanelState {
                mode: 1,
                resource_uri: "musica:/sys/custom.png".into(),
            }),
        ),
        (
            2,
            Some(MusicaPanelState {
                mode: 3,
                resource_uri: "musica:/sys/fullPanel.png".into(),
            }),
        ),
        (3, None),
    ] {
        assert!(
            matches!(vm.step(tick).unwrap(), Some(MusicaVmEvent::Panel { sequence }) if sequence == tick)
        );
        assert_eq!(vm.state().panel, expected);
        let restored = MusicaVm::decode_native_save(&vm.encode_native_save().unwrap()).unwrap();
        assert_eq!(restored.panel, expected);
    }
}

#[test]
fn musica_effect_end_rejects_operands_without_clearing_the_live_effect() {
    let source = b".effect CrossFade2 first.png:second.png 32 100\r\n.effect end extra\r\n";
    let mut vm = MusicaVm::new(
        "musica:/scr/test.sc".into(),
        Hash256::from_sha256(source),
        parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
        1,
    )
    .unwrap();
    vm.step(1).unwrap();
    vm.advance_effect_clock(100_000_000).unwrap();
    let effect = vm.state().effect.clone();
    let sequence = vm.state().effect_sequence;
    assert_eq!(vm.step(2).unwrap_err(), MusicaRuntimeError::Effect);
    assert_eq!(vm.state().effect, effect);
    assert_eq!(vm.state().effect_sequence, sequence);
}
