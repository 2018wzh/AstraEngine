use super::*;
use std::collections::BTreeSet;
const MAX_FIELDS: usize = 4096;
const MAX_RECORDS: usize = 16384;
const MAX_LINE: usize = 64 * 1024;
type Fields = Vec<EdenSaveField>;
fn field(line: &str) -> Result<EdenSaveField, CoreError> {
    let (name, value) = line
        .split_once('\t')
        .ok_or_else(|| invalid("BODY", "save field separator is missing"))?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
        || line.len() > MAX_LINE
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(invalid("BODY", "save field is invalid"));
    }
    Ok(EdenSaveField {
        name: name.into(),
        value: value.into(),
    })
}
fn push(fields: &mut Fields, names: &mut BTreeSet<String>, line: &str) -> Result<(), CoreError> {
    let field = field(line)?;
    if fields.len() >= MAX_FIELDS || !names.insert(field.name.clone()) {
        return Err(invalid(
            "BODY",
            "save fields are duplicate or exceed the count limit",
        ));
    }
    fields.push(field);
    Ok(())
}
pub(super) fn parse(text: &str) -> Result<(Fields, Vec<Fields>), CoreError> {
    if !text.ends_with('\n') || text.contains('\r') {
        return Err(invalid("BODY", "save body line endings are invalid"));
    }
    let mut lines = text.split_terminator('\n');
    if lines.next() != Some("<begin variables>") {
        return Err(invalid("BODY", "save variables section is missing"));
    }
    let mut variables = Vec::new();
    let mut names = BTreeSet::new();
    let mut ended = false;
    for line in lines.by_ref() {
        if line == "<end variables>" {
            ended = true;
            break;
        }
        let line = line
            .strip_prefix('!')
            .ok_or_else(|| invalid("BODY", "save variable marker is missing"))?;
        push(&mut variables, &mut names, line)?;
    }
    if !ended || variables.is_empty() || lines.next() != Some("<<begin backlog>>") {
        return Err(invalid("BODY", "save sections are missing or out of order"));
    }
    let mut backlog = Vec::new();
    let mut record = Vec::new();
    names.clear();
    ended = false;
    for line in lines.by_ref() {
        if line == "<<end backlog>>" {
            ended = true;
            break;
        }
        if line == "ZZ" {
            if record.is_empty() || backlog.len() >= MAX_RECORDS {
                return Err(invalid("BOUND", "save backlog record count is invalid"));
            }
            backlog.push(std::mem::take(&mut record));
            names.clear();
        } else {
            push(&mut record, &mut names, line)?;
        }
    }
    if !ended || !record.is_empty() || lines.next().is_some() {
        return Err(invalid(
            "BODY",
            "save backlog is incomplete or has trailing data",
        ));
    }
    Ok((variables, backlog))
}
fn append(text: &mut String, item: &EdenSaveField, prefix: &str) -> Result<(), CoreError> {
    let length = item
        .name
        .len()
        .saturating_add(item.value.len())
        .saturating_add(2);
    if length > MAX_LINE
        || text
            .len()
            .saturating_add(length)
            .saturating_add(prefix.len())
            > MAX_BODY
    {
        return Err(invalid(
            "BOUND",
            "save body or field exceeds the byte limit",
        ));
    }
    text.push_str(prefix);
    text.push_str(&item.name);
    text.push('\t');
    text.push_str(&item.value);
    text.push('\n');
    Ok(())
}
pub(super) fn format(save: &EdenSave) -> Result<String, CoreError> {
    if save.variables.len() > MAX_FIELDS
        || save.backlog.len() > MAX_RECORDS
        || save.backlog.iter().any(|record| record.len() > MAX_FIELDS)
    {
        return Err(invalid(
            "BOUND",
            "save field or record count exceeds the limit",
        ));
    }
    let mut text = String::from("<begin variables>\n");
    for item in &save.variables {
        append(&mut text, item, "!")?;
    }
    text.push_str("<end variables>\n<<begin backlog>>\n");
    for fields in &save.backlog {
        for item in fields {
            append(&mut text, item, "")?;
        }
        text.push_str("ZZ\n");
    }
    text.push_str("<<end backlog>>\n");
    let (variables, backlog) = parse(&text)?;
    if variables != save.variables || backlog != save.backlog {
        return Err(invalid(
            "BODY",
            "save field contains a structural delimiter",
        ));
    }
    Ok(text)
}
