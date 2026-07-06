local policy = astra.policy.current()

policy:on("runtime_start", function(ctx)
  astra.trace.event("advanced_policy.loaded", {
    profile = "vn.advanced_presentation",
  })
end)

astra.command.register("cinematic.reveal", {
  schema = "astra.command.text_effect.cinematic_reveal.v1",
  snapshot = "serializable",
  skip = "snap_to_end",
  replay = "deterministic",
  editor = {
    inspector = true,
    timeline_track = "text_effect",
  },
}, function(params)
  astra.mutate.timeline({
    task_id = params.task_id,
    command_id = params.command_id,
    join_policy = "wait_all",
    skip_policy = "snap_to_end",
    fallback = {
      if_filter_missing = "plain_reveal",
    },
    tracks = {
      {
        kind = "text_effect",
        target = params.target,
        preset = "cinematic_reveal",
      },
    },
  })
end)

policy:on("release_check", function(report)
  report:require("vn.advanced_presentation", {
    fields = {
      "timeline.join_cancel",
      "voice.sync",
      "presentation.fallback",
      "renderer.effect_budget",
    },
  })
end)

return policy
