use crate::ContextOutput;
use crate::app::command::OutputMode;
use crate::app::output::CommandOutput;

#[test]
fn context_human_render_is_readable() {
    let output = CommandOutput::Context(ContextOutput {
        goal: "find release blockers".to_string(),
        hits: vec![crate::ContextHit {
            handle: "plan.md".to_string(),
            span_id: Some("body".to_string()),
            score: 0.989_999_949_932_098_4,
            reason: "body:release".to_string(),
            field: "body".to_string(),
            summary: Some("Release".to_string()),
            status: Some("active".to_string()),
            disposition: "current_head".to_string(),
            age_days: Some(12),
            topic_signal: "siblings".to_string(),
            newer_topic_sibling_count: 2,
            top_newer_topic_sibling: Some("next.md".to_string()),
        }],
        spans: vec![crate::ContextSpan {
            handle: "plan.md".to_string(),
            span_id: "body".to_string(),
            start_line: 10,
            end_line: 12,
            tokens: 12,
            text: Some("Release blocker details.\nNext line.".to_string()),
        }],
        neighborhood: vec![crate::ContextNeighbor {
            handle: "plan.md".to_string(),
            neighbor: "dep.md".to_string(),
            status: Some("active".to_string()),
            disposition: "current".to_string(),
            age_days: Some(3),
            degree: 4,
            group: "current".to_string(),
        }],
    });
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::Human)
        .expect("render context");
    let rendered = String::from_utf8(rendered).expect("utf8");

    assert!(rendered.contains("Context\nGoal: find release blockers"));
    assert!(rendered.contains("Hits\n 1. plan.md"));
    assert!(rendered.contains("disposition=current_head status=active age_days=12"));
    assert!(rendered.contains("2 unmarked newer topical siblings (top: next.md; follow-up: anneal -e '? currency_suspect(\"plan.md\", newer).')"));
    assert!(rendered.contains("summary=Release"));
    assert!(rendered.contains("Read\nplan.md span=body lines=10-12 tokens=12"));
    assert!(rendered.contains("Neighborhood\nplan.md:\n  current: dep.md disposition=current status=active age_days=3 degree=4"));
}

#[test]
fn context_json_render_streams_event_rows() {
    let output = CommandOutput::Context(ContextOutput {
        goal: "find release blockers".to_string(),
        hits: vec![crate::ContextHit {
            handle: "plan.md".to_string(),
            span_id: Some("body".to_string()),
            score: 0.989_999_949_932_098_4,
            reason: "body:release".to_string(),
            field: "body".to_string(),
            summary: Some("Release".to_string()),
            status: Some("active".to_string()),
            disposition: "current_head".to_string(),
            age_days: Some(12),
            topic_signal: "siblings".to_string(),
            newer_topic_sibling_count: 2,
            top_newer_topic_sibling: Some("next.md".to_string()),
        }],
        spans: vec![crate::ContextSpan {
            handle: "plan.md".to_string(),
            span_id: "body".to_string(),
            start_line: 10,
            end_line: 12,
            tokens: 12,
            text: None,
        }],
        neighborhood: vec![crate::ContextNeighbor {
            handle: "plan.md".to_string(),
            neighbor: "dep.md".to_string(),
            status: Some("active".to_string()),
            disposition: "current".to_string(),
            age_days: Some(3),
            degree: 4,
            group: "current".to_string(),
        }],
    });
    let mut rendered = Vec::new();

    output
        .write(&mut rendered, OutputMode::JsonExplicit)
        .expect("render context");
    let rendered = String::from_utf8(rendered).expect("utf8");
    let rows = rendered
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json row"))
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["section"], "goal");
    assert_eq!(rows[0]["goal"], "find release blockers");
    assert_eq!(rows[1]["section"], "hit");
    assert_eq!(rows[1]["handle"], "plan.md");
    assert_eq!(rows[1]["score"].to_string(), "0.99");
    assert_eq!(rows[1]["disposition"], "current_head");
    assert_eq!(rows[1]["status"], "active");
    assert_eq!(rows[1]["age_days"], 12);
    assert_eq!(rows[1]["topic_signal"], "siblings");
    assert_eq!(rows[1]["newer_topic_sibling_count"], 2);
    assert_eq!(rows[1]["top_newer_topic_sibling"], "next.md");
    assert_eq!(rows[2]["section"], "span");
    assert_eq!(rows[2]["span_id"], "body");
    assert!(rows[2].get("text").is_none());
    assert_eq!(rows[3]["section"], "neighbor");
    assert_eq!(rows[3]["neighbor"], "dep.md");
    assert_eq!(rows[3]["disposition"], "current");
    assert_eq!(rows[3]["status"], "active");
    assert_eq!(rows[3]["age_days"], 3);
    assert_eq!(rows[3]["degree"], 4);
    assert_eq!(rows[3]["group"], "current");
}
