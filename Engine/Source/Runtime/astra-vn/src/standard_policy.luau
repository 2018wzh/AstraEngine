local pages = {
  "title",
  "save",
  "load",
  "config",
  "gallery",
  "replay",
  "voice_replay",
  "route_chart",
  "backlog",
  "localization_preview",
}

astra.command.register("astra.vn.message.show", {
  schema = "astra.vn.command.message.show.v1",
  role = "message_ui",
  budget_ms = 1,
}, function(command)
  return command ~= nil
end)

astra.command.register("astra.vn.choice.show", {
  schema = "astra.vn.command.choice.show.v1",
  role = "choice_ui",
  budget_ms = 1,
}, function(command)
  return command ~= nil
end)

astra.command.register("astra.vn.system.open", {
  schema = "astra.vn.command.system.open.v1",
  role = "system_story",
  budget_ms = 2,
}, function(command)
  return command ~= nil
end)

astra.command.register("astra.vn.timeline.run", {
  schema = "astra.vn.command.timeline.run.v1",
  role = "presentation_timeline",
  budget_ms = 2,
}, function(command)
  return command ~= nil
end)

astra.snapshot.set("astra.policy.standard", {
  version = 1,
  system_page_count = #pages,
  message_ui = true,
  choice_ui = true,
})

astra.trace.event("policy.loaded", {
  id = "astra.policy.standard",
  system_page_count = #pages,
})

return astra.snapshot.get("astra.policy.standard").system_page_count == #pages
