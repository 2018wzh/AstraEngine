local policy = astra.policy.define {
  id = "com.example.cinematic",
  version = "0.1.0",
  rust_plugin = "com.example.cinematic_nodes",
}

policy:command("astra.cinematic.reveal_text", {
  params = {
    speed = "number",
    lip_sync = "bool",
  },
  editor = {
    node = "Cinematic Reveal",
    inspector = "text_effect",
    timeline_track = "text_effect",
    preview = "message_window",
  },
  budget_us = 500,
  mutation_scope = { "presentation", "timeline" },
}, function(ctx, params)
  astra.mutate.timeline {
    kind = "text_reveal",
    speed = params.speed,
    lip_sync = params.lip_sync,
    fence = "text.reveal.done",
  }
end)

policy:command("astra.cinematic.camera_pulse", {
  params = {
    intensity = "number",
    duration = "u32",
  },
  editor = {
    node = "Camera Pulse",
    inspector = "camera",
    timeline_track = "camera",
    preview = "stage",
  },
  budget_us = 250,
  mutation_scope = { "presentation" },
}, function(ctx, params)
  astra.mutate.presentation {
    kind = "camera_pulse",
    intensity = params.intensity,
    duration = params.duration,
  }
end)

policy:on("render_frame", function(ctx)
  astra.trace.performance_scope("cinematic.render_frame")
end)

return policy
