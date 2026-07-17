use strsim::jaro_winkler;
use unicode_normalization::UnicodeNormalization;

use crate::{MatchAssessment, MatchEvidence, MetadataRecord};

pub const MATCHER_VERSION: &str = "astra.emu.metadata_matcher.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchInput {
    pub title: String,
    pub aliases: Vec<String>,
    pub developer: Option<String>,
    pub release_date: Option<String>,
    pub installation_fingerprint: String,
    pub previously_verified_remote_id: Option<String>,
    pub user_verified_remote_id: Option<String>,
}

pub fn normalize_title(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_space = true;
    for character in value.nfkc().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            normalized.push(character);
            last_space = false;
        } else if !last_space {
            normalized.push(' ');
            last_space = true;
        }
    }
    normalized.trim().to_owned()
}

pub fn match_metadata(input: &MatchInput, record: &MetadataRecord) -> MatchAssessment {
    let local_titles = std::iter::once(&input.title)
        .chain(input.aliases.iter())
        .map(|value| normalize_title(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let remote_titles = std::iter::once(&record.title)
        .chain(record.alternate_titles.iter())
        .map(|value| normalize_title(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let mut evidence = Vec::new();
    let mut best_title = 0u16;
    for local in &local_titles {
        for remote in &remote_titles {
            let score = if local == remote {
                700
            } else {
                (jaro_winkler(local, remote) * 620.0).round() as u16
            };
            if score > best_title {
                best_title = score;
                evidence.retain(|item: &MatchEvidence| item.kind != "title");
                evidence.push(MatchEvidence {
                    kind: "title".into(),
                    local_value: local.clone(),
                    remote_value: remote.clone(),
                    score_millis: score,
                });
            }
        }
    }

    let developer_score = input
        .developer
        .as_ref()
        .and_then(|local| {
            let local = normalize_title(local);
            record
                .developers
                .iter()
                .map(|remote| (remote, normalize_title(remote)))
                .max_by_key(|(_, remote)| (jaro_winkler(&local, remote) * 1000.0) as u16)
                .map(|(raw, remote)| (local, raw, remote))
        })
        .map(|(local, raw, remote)| {
            let score = if local == remote { 180 } else { 0 };
            if score > 0 {
                evidence.push(MatchEvidence {
                    kind: "developer".into(),
                    local_value: local,
                    remote_value: raw.clone(),
                    score_millis: score,
                });
            }
            score
        })
        .unwrap_or(0);

    let date_score = match (&input.release_date, &record.release_date) {
        (Some(local), Some(remote)) if local == remote => {
            evidence.push(MatchEvidence {
                kind: "release_date".into(),
                local_value: local.clone(),
                remote_value: remote.clone(),
                score_millis: 120,
            });
            120
        }
        (Some(local), Some(remote))
            if local.get(..4).is_some() && local.get(..4) == remote.get(..4) =>
        {
            evidence.push(MatchEvidence {
                kind: "release_year".into(),
                local_value: local.clone(),
                remote_value: remote.clone(),
                score_millis: 60,
            });
            60
        }
        _ => 0,
    };

    let score_millis = (best_title + developer_score + date_score).min(1000);
    let deterministic = input
        .user_verified_remote_id
        .as_deref()
        .or(input.previously_verified_remote_id.as_deref())
        .is_some_and(|remote_id| remote_id == record.remote_id);
    if deterministic {
        evidence.push(MatchEvidence {
            kind: if input.user_verified_remote_id.is_some() {
                "user_verified_id"
            } else {
                "historical_verified_id"
            }
            .into(),
            local_value: input.installation_fingerprint.clone(),
            remote_value: record.remote_id.clone(),
            score_millis: 1000,
        });
    }

    MatchAssessment {
        matcher_version: MATCHER_VERSION.into(),
        score_millis: if deterministic { 1000 } else { score_millis },
        evidence,
        requires_confirmation: !deterministic,
        auto_link_eligible: deterministic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MetadataProviderId;

    fn record() -> MetadataRecord {
        MetadataRecord {
            provider: MetadataProviderId::Vndb,
            remote_id: "v1".into(),
            title: "ＡＩＲ".into(),
            alternate_titles: vec!["Air Standard Edition".into()],
            developers: vec!["Key".into()],
            release_date: Some("2000-09-08".into()),
            platforms: vec!["win".into()],
            engine: None,
            cover: None,
            sensitive: false,
        }
    }

    #[test]
    fn unicode_title_match_still_requires_confirmation() {
        let assessment = match_metadata(
            &MatchInput {
                title: "AIR".into(),
                aliases: Vec::new(),
                developer: Some("Key".into()),
                release_date: Some("2000-09-08".into()),
                installation_fingerprint: "ifp-v1-a".into(),
                previously_verified_remote_id: None,
                user_verified_remote_id: None,
            },
            &record(),
        );
        assert_eq!(normalize_title("ＡＩＲ"), "air");
        assert!(assessment.score_millis >= 900);
        assert!(assessment.requires_confirmation);
        assert!(!assessment.auto_link_eligible);
    }

    #[test]
    fn only_verified_id_is_auto_link_eligible() {
        let assessment = match_metadata(
            &MatchInput {
                title: "wrong".into(),
                aliases: Vec::new(),
                developer: None,
                release_date: None,
                installation_fingerprint: "ifp-v1-a".into(),
                previously_verified_remote_id: Some("v1".into()),
                user_verified_remote_id: None,
            },
            &record(),
        );
        assert!(assessment.auto_link_eligible);
        assert!(!assessment.requires_confirmation);
    }
}
