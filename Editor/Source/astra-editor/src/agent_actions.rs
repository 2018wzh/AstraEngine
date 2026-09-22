use super::*;

impl Editor {
    pub(super) fn cancel_agent(&mut self) {
        if let Some(permission) = self.permission.take() {
            let _ = permission.reply.send(RequestPermissionOutcome::Cancelled);
        }
        self.bridge.cancel();
        self.agent.cancel();
        if let Some(reply) = self.pending_reply.take() {
            let _ = reply.send(Err(anyhow::anyhow!("Cancelled by user")));
        }
        let _ = self.agent.begin(self.mode);
    }

    pub(super) fn prompt_agent(&mut self, cx: &mut Context<Self>) {
        if self
            .agent_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            self.status = "Wait for the active agent or cancel it".into();
            return;
        }
        let Some(command) = self.agent_command.clone() else {
            self.status = "Configure an external agent with --agent".into();
            return;
        };
        let prompt = self.agent_prompt.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        self.cancel_agent();
        self.agent_output.clear();
        let source = self.project.source_path().to_path_buf();
        let bridge = self.bridge.clone();
        let generation = bridge.generation();
        self.agent_task = Some(self.runtime.spawn(async move {
            let result = astra_editor::acp::prompt(command, prompt, source, bridge.clone()).await;
            bridge
                .message(
                    generation,
                    match result {
                        Ok(()) => "\nAgent turn ended".into(),
                        Err(error) => format!("\nAgent failed: {error}"),
                    },
                )
                .await;
        }));
    }

    pub(super) fn review(&mut self, approve: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.agent.resolve(approve, &mut self.project.documents);
        if let Some(reply) = self.pending_reply.take() {
            let _ = reply.send(if approve {
                result
            } else {
                Err(anyhow::anyhow!("User rejected batch"))
            });
        }
        self.sync(window, cx);
    }
}
