//! Ordering policy for the CLI context projection.

pub(crate) const NEIGHBOR_GROUP_CURRENT: &str = "current";
pub(crate) const NEIGHBOR_GROUP_IN_FLIGHT: &str = "in_flight";
pub(crate) const NEIGHBOR_GROUP_SUPERSEDED: &str = "superseded";
pub(crate) const NEIGHBOR_GROUP_HIDDEN: &str = "hidden";

pub(crate) fn hit_score(score: f64, reason: &str, field: &str) -> f64 {
    score + reason_bonus(reason) + field_bonus(field)
}

pub(crate) fn neighbor_score(group: &str, disposition: &str, degree: i64, is_self: bool) -> f64 {
    neighbor_group_bonus(group)
        + neighbor_disposition_bonus(disposition)
        + neighbor_self_bonus(is_self)
        - neighbor_degree_penalty(degree)
}

fn neighbor_group_bonus(group: &str) -> f64 {
    match group {
        NEIGHBOR_GROUP_CURRENT => 300.0,
        NEIGHBOR_GROUP_IN_FLIGHT => 200.0,
        NEIGHBOR_GROUP_SUPERSEDED => 100.0,
        NEIGHBOR_GROUP_HIDDEN => 0.0,
        _ => 150.0,
    }
}

fn neighbor_disposition_bonus(disposition: &str) -> f64 {
    match disposition {
        "current_head" => 35.0,
        "current" => 20.0,
        "superseded" => -40.0,
        _ => 0.0,
    }
}

fn neighbor_self_bonus(is_self: bool) -> f64 {
    if is_self { 50.0 } else { 0.0 }
}

fn neighbor_degree_penalty(degree: i64) -> f64 {
    match degree.max(0) {
        0..=9 => 0.0,
        10..=24 => 8.0,
        25..=49 => 16.0,
        50..=99 => 28.0,
        100..=249 => 40.0,
        _ => 55.0,
    }
}

fn reason_bonus(reason: &str) -> f64 {
    match reason {
        "parent-cluster" => 0.250,
        _ => 0.0,
    }
}

fn field_bonus(field: &str) -> f64 {
    match field {
        "heading" => 0.040,
        "body" => 0.015,
        "title" | "identifier" => 0.005,
        field if field.starts_with("frontmatter:") => 0.002,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_order_prefers_structural_and_direct_content_signals() {
        let base = hit_score(0.8, "body-substring", "body");
        let heading = hit_score(0.8, "heading-substring", "heading");
        let clustered = hit_score(0.8, "parent-cluster", "identifier");
        let frontmatter = hit_score(0.8, "frontmatter-value-match", "frontmatter:status");

        assert!(
            clustered > heading && heading > base && base > frontmatter,
            "context ordering should prefer clustered canonical hits, then headings, then body, then frontmatter"
        );
    }
}
