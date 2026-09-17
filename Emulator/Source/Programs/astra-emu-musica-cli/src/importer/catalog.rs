use super::*;

/// List dictionary keys that name PAZ schemes, without serializing scheme data.
/// Listing does not imply that a scheme version is supported by the importer.
pub fn list_titles(formats: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let graph = load_graph(formats)?;
    let root = graph.root().map_err(|_| "ASTRA_EMU_GARBRO_ROOT")?;
    let mut titles = BTreeSet::new();
    visit_dictionary_pairs(&graph, root, |title, value| {
        let value = graph
            .dereference(value)
            .map_err(|_| "ASTRA_EMU_GARBRO_REFERENCE")?;
        if matches!(value, NrbfValue::Object(object) if object.class == "GameRes.Formats.Musica.PazScheme")
        {
            if title.is_empty() || title.len() > 512 {
                return Err("ASTRA_EMU_GARBRO_TITLE".into());
            }
            if !titles.insert(title.to_owned()) {
                return Err("ASTRA_EMU_GARBRO_TITLE_DUPLICATE".into());
            }
            if titles.len() > MAX_DICTIONARY_ENTRIES {
                return Err("ASTRA_EMU_GARBRO_DICTIONARY_LIMIT".into());
            }
        }
        Ok(true)
    })?;
    Ok(titles.into_iter().collect())
}
