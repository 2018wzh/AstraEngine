aivn.extension("live2d", "1.0.0")

aivn.command("motion.play", {
  version = 1,
  params = {
    actor = { type = "ActorRef", required = true },
    motion = { type = "Asset<Motion>", required = true },
    mix = { type = "Duration", default = "200ms" }
  },
  execution = {
    kind = "cue",
    blocking = true,
    deterministic = true,
    save = "serializable",
    skip = "finish",
    rollback = "snapshot",
    channels = { "actor:{actor}.live2d.motion" }
  },
  editor = {
    label = "Live2D Motion",
    category = "Live2D/Motion",
    timelineTrack = "Live2D Motion"
  }
})
