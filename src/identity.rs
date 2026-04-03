use crate::checks::{Diagnostic, Severity};

#[allow(dead_code)]
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[allow(dead_code)]
fn stable_id(prefix: &str, payload: &str) -> String {
    format!("{prefix}_{:016x}", fnv1a_64(payload.as_bytes()))
}

#[allow(dead_code)]
fn canonical_diagnostic_payload(diagnostic: &Diagnostic) -> String {
    let evidence = diagnostic
        .evidence
        .as_ref()
        .map(|value| serde_json::to_string(value).expect("evidence serializes"))
        .unwrap_or_default();
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        diagnostic.severity as u8,
        diagnostic.code,
        diagnostic.file.as_deref().unwrap_or_default(),
        diagnostic.line.unwrap_or_default(),
        diagnostic.message,
        evidence,
        u8::from(matches!(diagnostic.severity, Severity::Suggestion)),
    )
}

#[allow(dead_code)]
pub(crate) fn diagnostic_id(diagnostic: &Diagnostic) -> String {
    stable_id("diag", &canonical_diagnostic_payload(diagnostic))
}

#[allow(dead_code)]
pub(crate) fn suggestion_id(diagnostic: &Diagnostic) -> Option<String> {
    matches!(diagnostic.severity, Severity::Suggestion)
        .then(|| stable_id("sugg", &canonical_diagnostic_payload(diagnostic)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::Evidence;

    fn sample_diagnostic() -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: "W002",
            message: "confidence gap".to_string(),
            file: Some("formal-model/v17.md".to_string()),
            line: Some(42),
            evidence: Some(Evidence::ConfidenceGap {
                source_status: "formal".to_string(),
                source_level: 3,
                target_status: "provisional".to_string(),
                target_level: 2,
            }),
        }
    }

    #[test]
    fn diagnostic_id_is_stable_for_same_input() {
        let diagnostic = sample_diagnostic();
        let first = diagnostic_id(&diagnostic);
        let second = diagnostic_id(&diagnostic);
        assert_eq!(first, second);
    }

    #[test]
    fn diagnostic_id_changes_when_payload_changes() {
        let first = sample_diagnostic();
        let mut second = sample_diagnostic();
        second.line = Some(43);
        assert_ne!(diagnostic_id(&first), diagnostic_id(&second));
    }

    #[test]
    fn suggestion_id_only_exists_for_suggestions() {
        let warning = sample_diagnostic();
        assert!(suggestion_id(&warning).is_none());

        let suggestion = Diagnostic {
            severity: Severity::Suggestion,
            code: "S001",
            message: "orphaned handle".to_string(),
            file: Some("OPEN-QUESTIONS.md".to_string()),
            line: Some(8),
            evidence: None,
        };
        let suggestion_id = suggestion_id(&suggestion).expect("suggestion id");
        assert!(suggestion_id.starts_with("sugg_"));
    }
}
