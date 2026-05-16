//! MCP surface crate for anneal v2.
//!
//! This crate is intentionally skeletal until the runtime and policy
//! surfaces land. Keeping it in the workspace now pins the public crate
//! topology before implementation spreads.

use anneal_core::{ActorContext, VerbDispatchError, VerbEntry, VerbRegistry, VerbRunPlan};

pub const SURFACE_NAME: &str = "anneal-mcp";

const STABLE_TOOLS: &[&str] = &[
    "eval",
    "search",
    "read",
    "verbs",
    "describe",
    "schema",
    "source_of",
    "dashboard",
    "run_verb",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpVerb {
    name: String,
    doc: String,
    output_schema: String,
}

impl McpVerb {
    fn from_entry(entry: &VerbEntry) -> Self {
        Self {
            name: entry.name().to_string(),
            doc: entry.doc().to_string(),
            output_schema: entry.output_schema().to_string(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn output_schema(&self) -> &str {
        &self.output_schema
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpToolCatalog {
    tools: Vec<&'static str>,
    verbs: Vec<McpVerb>,
}

impl McpToolCatalog {
    pub fn from_registry(registry: &VerbRegistry) -> Self {
        Self {
            tools: STABLE_TOOLS.to_vec(),
            verbs: registry.iter().map(McpVerb::from_entry).collect(),
        }
    }

    pub fn tools(&self) -> &[&'static str] {
        &self.tools
    }

    pub fn verbs(&self) -> &[McpVerb] {
        &self.verbs
    }

    pub fn run_verb(
        &self,
        registry: &VerbRegistry,
        actor: &ActorContext,
        name: &str,
    ) -> Result<VerbRunPlan, VerbDispatchError> {
        registry.run_plan_for_actor(name, actor)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use anneal_core::runtime::{parse_prelude_program, parse_program};
    use anneal_core::{ActorContext, VerbDispatchError, VerbLayer, VerbRegistry};

    use super::*;

    #[test]
    fn mcp_tool_surface_stays_stable_when_verbs_grow() {
        let prelude = parse_prelude_program(
            "views.dl",
            r#"
            @verb(name: "work", query: "? prelude_work(h).", doc: "Prelude work.", output_schema: "{\"h\":\"String\"}", default_args: [], capabilities: ["read"]).
            prelude_work("p").
            "#,
        )
        .expect("prelude parses");
        let mut project_source = r#"
        @verb(name: "work", query: "? project_work(h).", doc: "Project work.", output_schema: "{\"h\":\"String\"}", default_args: [], capabilities: ["read"]).
        project_work("p").
        "#
        .to_string();
        for index in 0..24 {
            write!(
                &mut project_source,
                r#"
                @verb(name: "project-{index}", query: "? project_item_{index}(h).", doc: "Project verb.", output_schema: "{{\"h\":\"String\"}}", default_args: [], capabilities: ["read"]).
                project_item_{index}("h-{index}").
                "#
            )
            .expect("write project verb");
        }
        let project = parse_program("anneal.dl", &project_source).expect("project parses");
        let registry = VerbRegistry::from_layers(&[
            (VerbLayer::Prelude, &prelude),
            (VerbLayer::Project, &project),
        ])
        .expect("registry builds");

        let catalog = McpToolCatalog::from_registry(&registry);
        assert_eq!(catalog.tools(), STABLE_TOOLS);
        assert!(catalog.verbs().len() > catalog.tools().len());
        assert_eq!(
            catalog
                .tools()
                .iter()
                .filter(|tool| **tool == "work" || tool.starts_with("project-"))
                .count(),
            0
        );
    }

    #[test]
    fn run_verb_dispatches_through_registry() {
        let project = parse_program(
            "anneal.dl",
            r#"
            @verb(name: "release", query: "? item(h).", doc: "Release.", output_schema: "{\"h\":\"String\"}", default_args: [], capabilities: ["release"]).
            item("h").
            "#,
        )
        .expect("project parses");
        let registry =
            VerbRegistry::from_layers(&[(VerbLayer::Project, &project)]).expect("registry builds");
        let catalog = McpToolCatalog::from_registry(&registry);

        assert!(matches!(
            catalog.run_verb(&registry, &ActorContext::anonymous_mcp(), "release"),
            Err(VerbDispatchError::CapabilityDenied { .. })
        ));
        let actor = ActorContext {
            actor: "host".to_string(),
            capabilities: BTreeSet::from(["release".to_string()]),
        };
        assert_eq!(
            catalog
                .run_verb(&registry, &actor, "release")
                .expect("capability admits")
                .query_source(),
            "? item(h)."
        );
    }
}
