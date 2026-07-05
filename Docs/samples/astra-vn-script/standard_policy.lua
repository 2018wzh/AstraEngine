local policy = astra.policy.define {
  id = "astra.policy.standard",
  version = "0.1.0",
}

policy:command("astra.vn.dialogue", {
  params = {
    key = "text_key",
    speaker = "speaker_id",
    voice = "asset_id?",
  },
  editor = {
    node = "Dialogue",
    inspector = "message_window",
    timeline_track = "text",
  },
  mutation_scope = { "backlog", "read_state", "presentation", "audio" },
}, function(ctx, params)
  local text = astra.query.text(params.key, ctx.locale)
  astra.mutate.push_backlog {
    key = params.key,
    speaker = params.speaker,
    voice = params.voice,
    text_hash = text.hash,
  }
  astra.mutate.presentation {
    kind = "message_window",
    key = params.key,
    speaker = params.speaker,
  }
  if params.voice then
    astra.mutate.audio { kind = "voice_play", asset = params.voice }
  end
end)

policy:command("astra.vn.choice", {
  params = { key = "text_key", options = "choice_option[]" },
  editor = { node = "Choice", inspector = "choice_list" },
  mutation_scope = { "choice", "read_state" },
}, function(ctx, params)
  astra.mutate.presentation {
    kind = "choice_window",
    key = params.key,
    options = params.options,
  }
end)

return policy
