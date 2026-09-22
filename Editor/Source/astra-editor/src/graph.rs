use astra_vn_editor::{parse_astra_source, AuthoringWorkspace, DocumentEdits, EditBatch, TextEdit};

/// Add a route only to a state without existing control flow. The source remains authoritative.
pub fn connect(
    workspace: &AuthoringWorkspace,
    path: &str,
    version: u64,
    state_id: &str,
    target: &str,
) -> anyhow::Result<EditBatch> {
    let document = workspace.document(path)?;
    anyhow::ensure!(
        document.version == version,
        "Graph changed; select the state again"
    );
    anyhow::ensure!(
        !target.is_empty()
            && target
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
        "Route target is not a source identifier"
    );
    let targets = workspace
        .documents()
        .flat_map(|document| {
            let parsed = parse_astra_source(&document.path, &document.text);
            parsed
                .ast
                .commands()
                .filter(|c| c.keyword() == "state")
                .filter_map(|c| c.arguments().next().map(|a| a.0.to_owned()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        targets
            .iter()
            .filter(|name| name.as_str() == target)
            .count()
            == 1,
        "Route target must name one existing state"
    );
    let parsed = parse_astra_source(path, &document.text);
    let commands = parsed.ast.commands().collect::<Vec<_>>();
    let index = commands
        .iter()
        .position(|c| c.keyword() == "state" && c.source_id() == Some(state_id))
        .ok_or_else(|| anyhow::anyhow!("State no longer exists"))?;
    let end_index = commands
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, c)| c.indent() <= commands[index].indent())
        .map(|(i, _)| i)
        .unwrap_or(commands.len());
    anyhow::ensure!(
        !commands[index + 1..end_index].iter().any(|c| matches!(
            c.keyword(),
            "jump" | "branch" | "choice" | "option" | "call" | "return"
        )),
        "State already has control flow; edit or remove its route first"
    );
    let at = commands
        .get(end_index)
        .map(|c| usize::from(c.keyword_span().start) - c.indent())
        .unwrap_or(document.text.len());
    let newline = if document.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = if at > 0 && !document.text[..at].ends_with('\n') {
        newline
    } else {
        ""
    };
    let scene = commands[index + 1..end_index]
        .iter()
        .rfind(|c| c.keyword() == "scene")
        .ok_or_else(|| anyhow::anyhow!("Add a scene to this state before connecting it"))?;
    let indent = " ".repeat(scene.indent() + 2);
    let id = uuid::Uuid::new_v4().simple();
    let replacement =
        format!("{prefix}{indent}jump target:{target} #@id editor.jump.{id}{newline}");
    Ok(EditBatch {
        documents: vec![DocumentEdits {
            path: path.into(),
            version,
            edits: vec![TextEdit {
                start: at,
                end: at,
                replacement,
            }],
        }],
    })
}

/// Delete the complete route command; a branch's two ports belong to one command.
/// Keep surrounding comments and any author comment following the source annotation.
pub fn remove(
    workspace: &AuthoringWorkspace,
    path: &str,
    version: u64,
    source_id: &str,
) -> anyhow::Result<EditBatch> {
    let document = workspace.document(path)?;
    anyhow::ensure!(
        document.version == version,
        "Graph changed; select the route again"
    );
    let parsed = parse_astra_source(path, &document.text);
    let command = parsed
        .ast
        .commands()
        .find(|c| c.source_id() == Some(source_id))
        .ok_or_else(|| anyhow::anyhow!("Route no longer exists"))?;
    anyhow::ensure!(
        matches!(command.keyword(), "jump" | "call" | "branch" | "option"),
        "Select a route command"
    );
    let mut start = usize::from(command.keyword_span().start);
    let id_end = usize::from(
        command
            .source_id_span()
            .ok_or_else(|| anyhow::anyhow!("Missing route source ID"))?
            .end,
    );
    let line_end = document.text[id_end..]
        .find('\n')
        .map(|offset| id_end + offset + 1)
        .unwrap_or(document.text.len());
    let end = if document.text[id_end..line_end].trim().is_empty() {
        start -= command.indent();
        line_end
    } else {
        id_end
    };
    Ok(EditBatch {
        documents: vec![DocumentEdits {
            path: path.into(),
            version,
            edits: vec![TextEdit {
                start,
                end,
                replacement: String::new(),
            }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_vn_editor::AstraSource;
    #[test]
    fn connect_remove_preserve_source_and_reject_stale_or_unknown_targets() {
        let source =
            "# note\nstory main #@id main\nstate start #@id start\n  scene entry #@id entry\nstate finish #@id finish\n  scene ending #@id ending\n";
        let mut workspace = AuthoringWorkspace::default();
        workspace
            .open(AstraSource::story("main.astra", source))
            .unwrap();
        assert!(connect(&workspace, "main.astra", 1, "start", "missing").is_err());
        let batch = connect(&workspace, "main.astra", 1, "start", "finish").unwrap();
        workspace.apply(batch).unwrap();
        workspace.compile(Default::default()).unwrap();
        assert!(connect(&workspace, "main.astra", 1, "start", "finish").is_err());
        assert!(connect(&workspace, "main.astra", 2, "start", "finish").is_err());
        let parsed = parse_astra_source(
            "main.astra",
            &workspace.document("main.astra").unwrap().text,
        );
        let id = parsed
            .ast
            .commands()
            .find(|c| c.keyword() == "jump")
            .unwrap()
            .source_id()
            .unwrap();
        workspace
            .apply(remove(&workspace, "main.astra", 2, id).unwrap())
            .unwrap();
        // Removing the only incoming route leaves an unreachable state; the normal
        // compiler diagnostic remains visible until the author reconnects it.
        assert!(workspace.compile(Default::default()).is_err());
        assert!(!workspace
            .document("main.astra")
            .unwrap()
            .text
            .lines()
            .any(|line| !line.is_empty() && line.trim().is_empty()));
        workspace.undo().unwrap();
        workspace.undo().unwrap();
        assert_eq!(workspace.document("main.astra").unwrap().text, source);
    }
}
