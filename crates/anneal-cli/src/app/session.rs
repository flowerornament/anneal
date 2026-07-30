//! Corpus session construction, evidence loading, and command execution.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use anneal_code::CodeSource;
use anneal_core::runtime::{
    AnalyzedProgram, Database, EvalOptions, Evaluator, Program, QueryOutput, Row, Statement, Value,
    analyze, parse_program,
};
use anneal_core::runtime::{ExplainOptions, NumberValue};
use anneal_core::runtime::{LoadedPrelude, PreludeError, datalog_string_literal};
use anneal_core::{
    ActorContext, CancellationToken, CodeDriftRefreshProgressSink, CodeTargetMeta, ConfigEntry,
    ConfigFacts, CorpusId, FactStore, Generation, ProjectExtension, SnapshotAppendOutcome,
    SnapshotEntry, SnapshotEntryFact, Source, SourceContext, SourceInfo, VerbEntry, VerbLayer,
    VerbRegistry, append_snapshot_entry_capped, load_project_extension, merge_program_layers,
    read_snapshot_history,
};
use anneal_md::{EdgeAssertionRefreshProgressSink, MarkdownSource};
use anyhow::{Context, Result, anyhow, bail, ensure};
use camino::Utf8PathBuf;
use chrono::{DateTime, SecondsFormat, Utc};

use crate::{ContextCommand, DescribeCommand, ReadCommand, SearchCommand};

use super::command::{DynamicVerbInvocation, RuntimeCommand, render_dynamic_verb_query};
use super::navigation::{handle_impact_rows, handle_lineage_rows};
use super::output::{
    CommandOutput, RankedAnchorEnrichment, RankedAnchorSignal, RowView, StatusOutput,
    partition_check_diagnostics, render_dynamic_verb_help, required_string,
    search_zero_result_hint,
};
use super::query_guidance::{
    authored_age_days, currency_disposition, empty_binding_example, ranked_anchor_handle_field,
    retired_section_kind_warning, warning_applies_to_query, warning_texts,
    zero_result_hint_for_query,
};

#[cfg(test)]
mod tests;

/// Corpus identity used by the single-process CLI runtime.
pub(super) const DEFAULT_CORPUS: &str = "cli";
/// Full diagnostic relation evaluated once before the renderer partitions it.
pub(super) const CHECK_DIAGNOSTIC_QUERY: &str =
    "? diagnostic(code, severity, subject, file, line, evidence).";
const DEFAULT_AUTO_SNAPSHOT_LIMIT: usize = 100;

fn drift_refresh_progress_sink() -> CodeDriftRefreshProgressSink {
    let last_reported = Mutex::new(Duration::ZERO);
    CodeDriftRefreshProgressSink::new(move |progress| {
        let mut last_reported = last_reported
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.completed == 0 {
            if progress.total == 0 {
                eprintln!("Drift refresh: cache is current (0 cold probes)");
            } else {
                eprintln!("Drift refresh: 0/{} cold probes", progress.total);
            }
            return;
        }
        if progress.completed == progress.total
            || progress.elapsed.saturating_sub(*last_reported) >= Duration::from_secs(1)
        {
            eprintln!(
                "Drift refresh: {}/{} cold probes ({:.1}s)",
                progress.completed,
                progress.total,
                progress.elapsed.as_secs_f64()
            );
            *last_reported = progress.elapsed;
        }
    })
}

fn edge_assertion_refresh_progress_sink() -> EdgeAssertionRefreshProgressSink {
    let last_reported = Mutex::new(Duration::ZERO);
    EdgeAssertionRefreshProgressSink::new(move |progress| {
        let mut last_reported = last_reported
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.completed == 0 {
            eprintln!(
                "Drift refresh: assertion provenance 0/{} lines",
                progress.total
            );
            return;
        }
        if progress.completed == progress.total
            || progress.elapsed.saturating_sub(*last_reported) >= Duration::from_secs(1)
        {
            eprintln!(
                "Drift refresh: assertion provenance {}/{} lines ({:.1}s)",
                progress.completed,
                progress.total,
                progress.elapsed.as_secs_f64()
            );
            *last_reported = progress.elapsed;
        }
    })
}

/// Installs CLI progress only for commands that explicitly refresh drift evidence.
pub(super) fn drift_refresh_progress_for(
    command: &RuntimeCommand,
) -> Option<CodeDriftRefreshProgressSink> {
    command
        .refreshes_code_drift_evidence()
        .then(drift_refresh_progress_sink)
}

/// Installs assertion-provenance progress only for explicit drift refreshes.
pub(super) fn edge_assertion_refresh_progress_for(
    command: &RuntimeCommand,
) -> Option<EdgeAssertionRefreshProgressSink> {
    command
        .refreshes_code_drift_evidence()
        .then(edge_assertion_refresh_progress_sink)
}

/// Returns the up-front cold-refresh notice for an explicit refresh command.
pub(super) fn drift_refresh_announcement(command: &RuntimeCommand) -> Option<&'static str> {
    command
        .refreshes_code_drift_evidence()
        .then_some("Drift refresh: collecting assertion provenance (this can take minutes)")
}

fn available_source_info() -> Vec<SourceInfo> {
    vec![MarkdownSource::default().describe(), CodeSource.describe()]
}

/// One immutable corpus snapshot plus the program and registries used to query it.
pub(super) struct RuntimeSession {
    root: Utf8PathBuf,
    program: Program,
    store: FactStore,
    registry: VerbRegistry,
    actor: ActorContext,
    sources: Vec<SourceInfo>,
    prelude_hash: String,
    git_mtimes: BTreeMap<String, String>,
}

struct CurrencyHitAnnotation {
    status: Option<String>,
    disposition: String,
    age_days: Option<i64>,
}

/// Built-in and project definitions shared by help routing and full sessions.
struct RuntimeDefinition {
    actor: ActorContext,
    sources: Vec<SourceInfo>,
    loaded_prelude: LoadedPrelude,
    project: Option<ProjectExtension>,
}

impl RuntimeDefinition {
    fn load(root: &camino::Utf8Path) -> Result<Self> {
        let actor = ActorContext::trusted_cli();
        let sources = available_source_info();
        let loaded_prelude = LoadedPrelude::load_active().map_err(prelude_error)?;
        if root.join("anneal.toml").is_file() {
            bail!(
                "anneal.toml is a legacy config file. Runtime commands use anneal.dl; run `anneal init --force` to write unified anneal.dl and move anneal.toml aside"
            );
        }
        let project = if root.join(anneal_core::PROJECT_RULE_FILE).is_file() {
            Some(load_project_extension(
                root.as_std_path(),
                &sources,
                loaded_prelude.program(),
            )?)
        } else {
            None
        };
        Ok(Self {
            actor,
            sources,
            loaded_prelude,
            project,
        })
    }

    fn verb_registry(&self) -> Result<VerbRegistry> {
        Ok(match &self.project {
            Some(project) => VerbRegistry::from_layers(&[
                (VerbLayer::Prelude, self.loaded_prelude.program()),
                (VerbLayer::Project, project.program()),
            ])?,
            None => {
                VerbRegistry::from_layers(&[(VerbLayer::Prelude, self.loaded_prelude.program())])?
            }
        })
    }

    fn described_names(&self) -> BTreeSet<String> {
        let mut names = described_program_names(self.loaded_prelude.program());
        names.extend(self.sources.iter().map(|source| source.name.to_string()));
        if let Some(project) = &self.project {
            names.extend(described_program_names(project.program()));
        }
        names
    }
}

/// Corpus-scoped verb registry used before the full fact session is loaded.
pub(super) struct RuntimeRegistry {
    registry: VerbRegistry,
    actor: ActorContext,
    described_names: BTreeSet<String>,
}

impl RuntimeRegistry {
    /// Loads the built-in and project verb layers needed for help-name routing.
    pub(super) fn load(root: &camino::Utf8Path) -> Result<Self> {
        let definition = RuntimeDefinition::load(root)?;
        let registry = definition.verb_registry()?;
        let described_names = definition.described_names();
        Ok(Self {
            registry,
            actor: definition.actor,
            described_names,
        })
    }

    /// Resolves a verb under the trusted CLI actor used to build this registry.
    pub(super) fn resolve(&self, name: &str) -> Result<&VerbEntry, anneal_core::VerbDispatchError> {
        self.registry.resolve_for_actor(name, &self.actor)
    }

    /// Returns whether runtime description metadata also owns this name.
    pub(super) fn has_described_name(&self, name: &str) -> bool {
        self.described_names.contains(name)
    }
}

fn described_program_names(program: &Program) -> BTreeSet<String> {
    fn collect(statements: &[Statement], names: &mut BTreeSet<String>) {
        for statement in statements {
            match statement {
                Statement::Rule(rule) => {
                    names.insert(rule.head.predicate.display_name());
                }
                Statement::Doc(doc) => {
                    names.insert(doc.name().to_string());
                }
                Statement::Predicate(decl) => {
                    if let Some(Ok(predicate)) = decl.predicate_ref() {
                        names.insert(predicate.display_name());
                    }
                }
                Statement::AtBlock { statements, .. } => collect(statements, names),
                Statement::Fact(_)
                | Statement::OptionalFact(_)
                | Statement::ConfigBlock(_)
                | Statement::SourceBlock(_)
                | Statement::Query(_)
                | Statement::Include(_)
                | Statement::Import(_)
                | Statement::Verb(_) => {}
            }
        }
    }

    let mut names = BTreeSet::new();
    collect(&program.statements, &mut names);
    names
}

impl RuntimeSession {
    /// Extracts the corpus once and builds the immutable database inputs for a command.
    pub(super) fn load(root: &camino::Utf8Path, command: &RuntimeCommand) -> Result<Self> {
        let definition = RuntimeDefinition::load(root)?;
        let registry = definition.verb_registry()?;
        let RuntimeDefinition {
            actor,
            sources,
            loaded_prelude,
            project,
        } = definition;
        let corpus = CorpusId::from(DEFAULT_CORPUS);
        let mut program = loaded_prelude.program().clone();
        let mut discovery = default_markdown_config();
        if let Some(project) = &project {
            merge_discovery(&mut discovery, project.discovery());
            let (merged, warnings) = merge_program_layers(program, project.program().clone());
            for warning in warnings {
                eprintln!(
                    "warning: {}:{}: '{}' overrides prelude ({} clauses)",
                    warning.location.source_name,
                    warning.location.line,
                    warning.predicate,
                    warning.replaced_clauses
                );
            }
            program = merged;
        }

        let runtime_config = project
            .as_ref()
            .map_or_else(ConfigFacts::default, |project| {
                project.runtime_config().clone()
            });
        let config_facts = ConfigFacts::from_entries(discovery);
        let mut markdown_source = MarkdownSource::with_runtime_config(&runtime_config)
            .map_err(|err| anyhow!("markdown config failed: {err}"))?;
        if let Some(progress) = drift_refresh_progress_for(command) {
            markdown_source = markdown_source.with_drift_refresh_progress(progress);
        }
        if let Some(progress) = edge_assertion_refresh_progress_for(command) {
            markdown_source = markdown_source.with_edge_assertion_refresh_progress(progress);
        }
        let code_source = CodeSource;
        let roots = vec![root.to_path_buf()];
        let context = SourceContext {
            corpus: corpus.clone(),
            roots: roots.as_slice(),
            config_facts: &config_facts,
            probe_code_target_history: command.demands_code_target_history(),
            read_code_drift_evidence: command.demands_code_drift_evidence(),
            refresh_code_drift_evidence: command.refreshes_code_drift_evidence(),
            probe_edge_assertions: command.demands_edge_assertions()
                || command.refreshes_code_drift_evidence(),
            time_ref: None,
            previous_generation: Some(Generation::new(0)),
            actor: actor.clone(),
            cancellation: CancellationToken::new(),
        };
        let markdown_batch = markdown_source
            .extract(&context)
            .map_err(|err| anyhow!("markdown extraction failed: {err}"))?;
        let mut store = FactStore::default();
        store
            .merge(markdown_batch)
            .context("failed to merge markdown facts")?;
        if CodeSource::is_configured(&config_facts) {
            let code_batch = code_source
                .extract(&context)
                .map_err(|err| anyhow!("code extraction failed: {err}"))?;
            store
                .merge(code_batch)
                .context("failed to merge code facts")?;
        }
        let configs = runtime_config_facts(project.as_ref(), &corpus);
        if !configs.is_empty() {
            store
                .replace_configs(&corpus, configs)
                .context("failed to merge runtime config facts")?;
        }
        let git_mtimes = git_mtimes_for_files(
            root,
            store.handles().iter().map(|handle| handle.file.as_str()),
        );
        let history = read_snapshot_history(root).context("failed to read snapshot history")?;
        store.replace_snapshot_history(&history);
        Ok(Self {
            root: root.to_path_buf(),
            program,
            store,
            registry,
            actor,
            sources,
            prelude_hash: loaded_prelude.set().hash().to_string(),
            git_mtimes,
        })
    }

    /// Returns the corpus root for executable-documentation fixtures.
    #[cfg(test)]
    pub(super) fn root(&self) -> &camino::Utf8Path {
        &self.root
    }

    #[cfg(test)]
    /// Loads the minimal session shape used by CLI unit and executable-doc tests.
    pub(super) fn load_for_test(root: &camino::Utf8Path) -> Result<Self> {
        Self::load(root, &RuntimeCommand::Schema)
    }

    #[cfg(test)]
    /// Verifies that a taught query parses and analyzes against the loaded vocabulary.
    pub(super) fn analyze_query_for_test(&self, query_source: &str) -> Result<()> {
        let analyzed = self.analyze_query_source("executable-doc-query", query_source)?;
        ensure!(
            analyzed.queries().next().is_some(),
            "query source did not contain a query"
        );
        Ok(())
    }
}

// Command dispatch remains a thin selection over registry plans and render views.
impl RuntimeSession {
    /// Executes a parsed command against this session without re-extracting the corpus.
    pub(super) fn run(&self, command: RuntimeCommand) -> Result<CommandOutput> {
        match command {
            RuntimeCommand::Status => self.run_status(),
            RuntimeCommand::Context {
                goal,
                budget,
                hits,
                depth,
                include_low_confidence,
                read_spans,
            } => {
                let command = ContextCommand::new(goal)
                    .with_budget(budget)
                    .with_hits(hits)
                    .with_neighborhood_depth(depth)
                    .include_low_confidence(include_low_confidence)
                    .read_spans(read_spans);
                let output = self.eval(command.datalog().as_str(), ExplainOptions::disabled())?;
                let output = command.group_rows(&output.rows)?;
                Ok(CommandOutput::Context(output))
            }
            RuntimeCommand::Search {
                query,
                limit,
                include_low_confidence,
            } => {
                let query = SearchCommand::new(query)
                    .with_limit(limit)
                    .include_low_confidence(include_low_confidence)
                    .datalog();
                let output = self.eval(&query, ExplainOptions::disabled())?;
                let mut rows = output.rows;
                self.annotate_search_rows(&mut rows);
                let zero_result_hint = rows
                    .is_empty()
                    .then(|| search_zero_result_hint(include_low_confidence));
                Ok(CommandOutput::rows_with_warnings(
                    rows,
                    RowView::Search,
                    warning_texts(&output.warnings),
                )
                .with_zero_result_hint(zero_result_hint))
            }
            RuntimeCommand::Read {
                handle,
                budget,
                span_id,
            } => {
                let query = ReadCommand::new(&handle)
                    .with_budget(budget)
                    .with_span_id(span_id)
                    .datalog();
                let output = self.eval(&query, ExplainOptions::disabled())?;
                let missing_handle =
                    if output.rows.is_empty() && !self.visible_handle_exists(&handle)? {
                        Some(handle)
                    } else {
                        None
                    };
                Ok(CommandOutput::rows_with_warnings(
                    output.rows,
                    RowView::Read { missing_handle },
                    warning_texts(&output.warnings),
                ))
            }
            RuntimeCommand::Handle {
                handle,
                impact,
                lineage,
            } => self.run_handle(handle, impact, lineage),
            RuntimeCommand::Check { .. } => self.run_check_gate(),
            RuntimeCommand::Describe { name } | RuntimeCommand::HelpName { name } => {
                self.run_describe(&name)
            }
            RuntimeCommand::Schema => self.run_verb("schema", RowView::Schema),
            RuntimeCommand::Eval {
                query,
                explain,
                limit,
            } => {
                let mut output = self.eval(&query, explain)?;
                if let Some(limit) = limit {
                    output.rows.truncate(limit);
                }
                let empty_binding_hint = self.empty_binding_hint_for_query(&query, &output.rows);
                let zero_result_hint = zero_result_hint_for_query(&query, &output.rows);
                let ranked_anchor = self.ranked_anchor_enrichment(&query, &output.rows)?;
                let view = ranked_anchor.as_ref().map_or(RowView::Eval, |enrichment| {
                    RowView::RankedAnchor {
                        handle_field: enrichment.handle_field.clone(),
                    }
                });
                Ok(CommandOutput::rows_with_ranked_anchor_enrichment(
                    output.rows,
                    view,
                    empty_binding_hint,
                    warning_texts(&output.warnings),
                    ranked_anchor,
                )
                .with_zero_result_hint(zero_result_hint))
            }
            RuntimeCommand::Verb { name, args } => self.run_dynamic_verb(&name, &args),
            RuntimeCommand::Help { topic } => Ok(CommandOutput::Text(topic.render())),
            RuntimeCommand::Version | RuntimeCommand::Init { .. } | RuntimeCommand::Prime => {
                bail!("command is handled before runtime session load")
            }
        }
    }

    fn run_verb(&self, name: &str, view: RowView) -> Result<CommandOutput> {
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        self.run_query(plan.query_source(), ExplainOptions::disabled(), view)
    }

    fn run_describe(&self, name: &str) -> Result<CommandOutput> {
        let query = DescribeCommand::new(name).datalog();
        let output = self.eval(&query, ExplainOptions::disabled())?;
        ensure!(
            !output.rows.is_empty(),
            "unknown runtime name {name:?}; use `anneal schema` or `anneal describe runtime`"
        );
        Ok(CommandOutput::rows(output.rows, RowView::Describe))
    }

    fn run_handle(&self, handle: String, impact: bool, lineage: bool) -> Result<CommandOutput> {
        let mut output = self.eval(&handle_query(&handle), ExplainOptions::disabled())?;
        if output.rows.is_empty() && looks_like_retired_section_handle(&handle) {
            bail!("{}", retired_section_handle_message(&handle));
        }
        if impact {
            output.rows.extend(self.handle_impact_rows(&handle));
        }
        if lineage {
            output
                .rows
                .extend(handle_lineage_rows(&self.store, &handle));
        }
        let missing = output.rows.is_empty();
        Ok(CommandOutput::rows(
            output.rows,
            RowView::Handle {
                handle,
                impact,
                lineage,
                missing,
            },
        ))
    }

    fn visible_handle_exists(&self, handle: &str) -> Result<bool> {
        let output = self.eval(&handle_query(handle), ExplainOptions::disabled())?;
        Ok(output
            .rows
            .iter()
            .any(|row| required_string(row, "relation").is_ok_and(|relation| relation == "self")))
    }

    fn handle_impact_rows(&self, handle: &str) -> Vec<Row> {
        handle_impact_rows(&self.store, handle)
    }

    fn run_check_gate(&self) -> Result<CommandOutput> {
        let output = self.eval(CHECK_DIAGNOSTIC_QUERY, ExplainOptions::disabled())?;
        let warnings = warning_texts(&output.warnings);
        let (error_rows, non_error_count) = partition_check_diagnostics(output.rows)?;
        let gate_failed = !error_rows.is_empty();
        let zero_result_hint = if gate_failed {
            None
        } else {
            Some(format!(
                "hint: check filters to error severity; {non_error_count} non-error diagnostic rows remain. Run `anneal -e '{CHECK_DIAGNOSTIC_QUERY}'`"
            ))
        };
        Ok(
            CommandOutput::rows_with_warnings(error_rows, RowView::Broken, warnings)
                .with_gate_failed(gate_failed)
                .with_zero_result_hint(zero_result_hint),
        )
    }

    /// Executes a registry verb against this loaded session.
    pub(super) fn run_dynamic_verb(&self, name: &str, args: &[String]) -> Result<CommandOutput> {
        self.run_dynamic_verb_with_view(name, args, None)
    }

    fn run_dynamic_verb_with_view(
        &self,
        name: &str,
        args: &[String],
        view: Option<RowView>,
    ) -> Result<CommandOutput> {
        let entry = self.registry.resolve_for_actor(name, &self.actor)?;
        let invocation = DynamicVerbInvocation::parse(entry, args)?;
        if invocation.help {
            return Ok(CommandOutput::Text(render_dynamic_verb_help(entry)));
        }
        let plan = self.registry.run_plan_for_actor(name, &self.actor)?;
        let query = render_dynamic_verb_query(plan.query_source(), &invocation.bindings);
        let mut output = self.eval(&query, invocation.explain)?;
        if let Some(rows) = invocation.rows {
            output.rows.truncate(rows);
        }
        let empty_binding_hint = self.empty_binding_hint_for_query(&query, &output.rows);
        let ranked_anchor = if plan.name().as_str() == "ranked_anchor" {
            self.ranked_anchor_enrichment(&query, &output.rows)?
        } else {
            None
        };
        let view = ranked_anchor.as_ref().map_or_else(
            || {
                view.unwrap_or_else(|| RowView::Verb {
                    name: plan.name().to_string(),
                })
            },
            |enrichment| RowView::RankedAnchor {
                handle_field: enrichment.handle_field.clone(),
            },
        );
        Ok(CommandOutput::rows_with_ranked_anchor_enrichment(
            output.rows,
            view,
            empty_binding_hint,
            warning_texts(&output.warnings),
            ranked_anchor,
        ))
    }

    /// Evaluates and renders the built-in status plan.
    pub(super) fn run_status(&self) -> Result<CommandOutput> {
        let snapshot_count_before = self.snapshot_history_count();
        let plan = self.registry.run_plan_for_actor("status", &self.actor)?;
        let output = self.eval(plan.query_source(), ExplainOptions::disabled())?;
        let append_outcome = match self.record_status_snapshot() {
            Ok(outcome) => Some(outcome),
            Err(err) => {
                eprintln!("warning: could not write automatic status snapshot: {err}");
                None
            }
        };
        let flow_baseline_ready = match append_outcome {
            Some(SnapshotAppendOutcome::Appended) if snapshot_count_before == 0 => false,
            _ => snapshot_count_before > 0,
        };
        Ok(CommandOutput::Status(StatusOutput {
            rows: output.rows,
            flow_baseline_ready,
        }))
    }
}

// Snapshot persistence and row enrichment derive presentation data from one loaded store.
impl RuntimeSession {
    fn record_status_snapshot(&self) -> Result<SnapshotAppendOutcome> {
        let entry = self.status_snapshot_entry();
        append_snapshot_entry_capped(&self.root, &entry, DEFAULT_AUTO_SNAPSHOT_LIMIT)
            .context("failed to append automatic status snapshot")
    }

    fn snapshot_history_count(&self) -> usize {
        self.store
            .snapshots()
            .iter()
            .map(|snapshot| snapshot.snapshot.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn status_snapshot_entry(&self) -> SnapshotEntry {
        let at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut facts = self
            .store
            .handles()
            .iter()
            .filter_map(|handle| {
                handle.status.as_ref().map(|status| {
                    SnapshotEntryFact::new(handle.id.clone(), "status", status.as_str())
                })
            })
            .collect::<Vec<_>>();
        facts.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.key.cmp(&right.key))
                .then_with(|| left.value.cmp(&right.value))
        });
        SnapshotEntry::with_prelude_hash(
            format!("status-{at}"),
            at,
            CorpusId::from(DEFAULT_CORPUS),
            self.prelude_hash.clone(),
            facts,
        )
    }

    fn run_query(
        &self,
        query: &str,
        explain: ExplainOptions,
        view: RowView,
    ) -> Result<CommandOutput> {
        let output = self.eval(query, explain)?;
        Ok(CommandOutput::rows_with_warnings(
            output.rows,
            view,
            warning_texts(&output.warnings),
        ))
    }

    fn annotate_search_rows(&self, rows: &mut [Row]) {
        let handles = rows
            .iter()
            .filter_map(|row| match row.fields.get("h") {
                Some(Value::String(handle)) => Some(handle.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let annotations = self.currency_hit_annotations(&handles);
        for row in rows {
            let Some(Value::String(handle)) = row.fields.get("h") else {
                continue;
            };
            let Some(annotation) = annotations.get(handle.as_str()) else {
                continue;
            };
            row.fields.insert(
                "status".to_string(),
                annotation
                    .status
                    .as_ref()
                    .map_or(Value::Null, |status| Value::String(status.clone())),
            );
            row.fields.insert(
                "disposition".to_string(),
                Value::String(annotation.disposition.clone()),
            );
            row.fields.insert(
                "age_days".to_string(),
                annotation
                    .age_days
                    .map_or(Value::Null, |days| Value::Number(NumberValue::Int(days))),
            );
        }
    }

    fn ranked_anchor_enrichment(
        &self,
        query: &str,
        rows: &[Row],
    ) -> Result<Option<RankedAnchorEnrichment>> {
        let Some(handle_field) = ranked_anchor_handle_field(query) else {
            return Ok(None);
        };
        let handles = rows
            .iter()
            .filter_map(|row| match row.fields.get(handle_field.as_str()) {
                Some(Value::String(handle)) => Some(handle.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let signals_by_handle = self.anchor_signals_for_handles(&handles)?;
        Ok(Some(RankedAnchorEnrichment {
            handle_field,
            signals_by_handle,
        }))
    }

    fn anchor_signals_for_handles(
        &self,
        handles: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, Vec<RankedAnchorSignal>>> {
        if handles.is_empty() {
            return Ok(BTreeMap::new());
        }

        let output = self.eval(
            "? anchor_signal(h, score, priority, why).",
            ExplainOptions::disabled(),
        )?;
        let mut by_handle = BTreeMap::<String, Vec<RankedAnchorSignal>>::new();
        for row in output.rows {
            let Some(Value::String(handle)) = row.fields.get("h") else {
                continue;
            };
            if !handles.contains(handle) {
                continue;
            }
            let Some(Value::Number(score)) = row.fields.get("score") else {
                continue;
            };
            let Some(Value::Number(NumberValue::Int(priority))) = row.fields.get("priority") else {
                continue;
            };
            let Some(Value::String(why)) = row.fields.get("why") else {
                continue;
            };
            by_handle
                .entry(handle.clone())
                .or_default()
                .push(RankedAnchorSignal {
                    why: why.clone(),
                    score: *score,
                    priority: *priority,
                });
        }
        for signals in by_handle.values_mut() {
            signals.sort_by(|left, right| {
                left.priority
                    .cmp(&right.priority)
                    .then_with(|| right.score.cmp(&left.score))
                    .then_with(|| left.why.cmp(&right.why))
            });
        }
        Ok(by_handle)
    }

    fn currency_hit_annotations(
        &self,
        handles: &BTreeSet<String>,
    ) -> BTreeMap<&str, CurrencyHitAnnotation> {
        let today = Utc::now().date_naive();
        let mut superseded = BTreeSet::new();
        let mut successors = BTreeSet::new();
        for edge in self.store.edges() {
            if edge.kind != "Supersedes" {
                continue;
            }
            if handles.contains(edge.from.as_str()) {
                superseded.insert(edge.from.as_str());
            }
            if handles.contains(edge.to.as_str()) {
                successors.insert(edge.to.as_str());
            }
        }
        self.store
            .handles()
            .iter()
            .filter(|handle| handles.contains(handle.id.as_str()))
            .map(|handle| {
                let age_days = handle
                    .date
                    .as_deref()
                    .and_then(|date| authored_age_days(date, today));
                let disposition = if handle.kind == "file" {
                    currency_disposition(handle.id.as_str(), &superseded, &successors)
                } else {
                    "unknown"
                };
                (
                    handle.id.as_str(),
                    CurrencyHitAnnotation {
                        status: handle.status.clone(),
                        disposition: disposition.to_string(),
                        age_days,
                    },
                )
            })
            .collect()
    }
}

// Query evaluation owns fixpoint construction and warnings derived from that same result.
impl RuntimeSession {
    /// Evaluates one query against this session's immutable database.
    pub(super) fn eval(&self, query_source: &str, explain: ExplainOptions) -> Result<QueryOutput> {
        let analyzed = self.analyze_query_source("cli-query", query_source)?;
        let query = analyzed
            .queries()
            .next()
            .cloned()
            .context("query source did not contain a query")?;
        let mut options = EvalOptions::default().with_actor(self.actor.clone());
        if explain.is_enabled() {
            options = options.with_explain_options(explain);
        }
        let database = Database::from_store_for_options(&self.store, &options)
            .with_sources(self.sources.clone())
            .with_git_mtimes(self.git_mtimes.clone());
        let mut evaluator = Evaluator::with_options(analyzed, database, options);
        evaluator
            .run_fixpoint_for_query(&query)
            .context("query fixpoint failed")?;
        let mut output = evaluator
            .eval_query(&query)
            .context("query evaluation failed")?;
        output
            .warnings
            .retain(|warning| warning_applies_to_query(query_source, warning));
        if let Some(warning) = retired_section_kind_warning(&query.query().body) {
            output.warnings.push(warning);
        }
        Ok(output)
    }

    /// Builds the empty-binding hint for rows returned by this session.
    pub(super) fn empty_binding_hint_for_query(
        &self,
        query_source: &str,
        rows: &[Row],
    ) -> Option<String> {
        if rows.is_empty() || rows.iter().any(|row| !row.fields.is_empty()) {
            return None;
        }
        let analyzed = self.analyze_query_source("cli-query", query_source).ok()?;
        let query = analyzed.queries().next()?.query();
        empty_binding_example(&analyzed, &query.body)
    }

    fn analyze_query_source(
        &self,
        source_name: &str,
        query_source: &str,
    ) -> Result<AnalyzedProgram> {
        let mut program = self.program.clone();
        let query_program = parse_program(source_name, query_source)
            .with_context(|| format!("failed to parse query {query_source:?}"))?;
        program.statements.extend(query_program.statements);
        analyze(program).context("query failed static analysis")
    }
}

fn runtime_config_facts(
    project: Option<&ProjectExtension>,
    corpus: &CorpusId,
) -> Vec<anneal_core::ConfigFact> {
    project.map_or_else(Vec::new, |project| project.runtime_config_facts(corpus))
}

fn git_mtimes_for_files<'a>(
    root: &camino::Utf8Path,
    files: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, String> {
    if !is_inside_git_work_tree(root) {
        return BTreeMap::new();
    }

    let files = files
        .into_iter()
        .filter(|file| !file.is_empty())
        .collect::<BTreeSet<_>>();
    if files.is_empty() {
        return BTreeMap::new();
    }

    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["log", "--relative", "--format=%cI", "--name-only", "--"])
        .arg(".")
        .output()
    else {
        return BTreeMap::new();
    };
    if !output.status.success() {
        return BTreeMap::new();
    }

    let mut mtimes = BTreeMap::new();
    let mut current_instant = None::<String>;
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
    {
        if line.is_empty() {
            continue;
        }
        if DateTime::parse_from_rfc3339(line).is_ok() {
            current_instant = Some(line.to_string());
            continue;
        }
        if files.contains(line)
            && !mtimes.contains_key(line)
            && let Some(instant) = &current_instant
        {
            mtimes.insert(line.to_string(), instant.clone());
            if mtimes.len() == files.len() {
                break;
            }
        }
    }
    mtimes
}

fn is_inside_git_work_tree(root: &camino::Utf8Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root.as_std_path())
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn default_markdown_config() -> Vec<ConfigEntry> {
    vec![
        ConfigEntry::scalar("md.file_extension", ".md"),
        ConfigEntry::scalar("md.scan_root", "."),
    ]
}

fn merge_discovery(discovery: &mut Vec<ConfigEntry>, extension: &ConfigFacts) {
    for entry in extension.entries() {
        if entry.ordinal.is_none() {
            discovery.retain(|existing| existing.key != entry.key || existing.ordinal.is_some());
        }
        discovery.push(entry.clone());
    }
}

/// Builds the visibility-respecting query used by handle and read surfaces.
pub(super) fn handle_query(handle: &str) -> String {
    let handle = datalog_string_literal(handle);
    let external_class = CodeTargetMeta::EXTERNAL_CLASS;
    let class_code = CodeTargetMeta::CLASS_CODE;
    let target_path = CodeTargetMeta::TARGET_PATH;
    let referent_disposition = CodeTargetMeta::REFERENT_DISPOSITION;
    let move_candidate_count = CodeTargetMeta::REFERENT_MOVE_CANDIDATE_COUNT;
    let moved_to = CodeTargetMeta::REFERENT_MOVED_TO;
    format!(
        r#"
handle_focus({handle}).

handle_row({handle}, "self", {handle}, kind, status, file, line, summary, null, null, null) :=
  *handle{{id: {handle}, kind: kind, status: status, file: file, line: line, summary: summary}}.

handle_row({handle}, "out", other, kind, null, file, line, "", null, null, null) :=
  *edge{{from: {handle}, to: other, kind: kind, file: file, line: line}},
  not code_reference(other).

handle_row({handle}, "code_ref", other, "Cites", null, file, line, target_path, disposition, candidate_count, moved_to) :=
  *edge{{from: {handle}, to: other, kind: "Cites", file: file, line: line}},
  *meta{{handle: other, key: "{external_class}", value: "{class_code}"}},
  *meta{{handle: other, key: "{target_path}", value: target_path}},
  code_ref_disposition(other, disposition),
  code_ref_candidate_count(other, candidate_count),
  code_ref_moved_to(other, moved_to).

handle_row({handle}, "in", other, kind, null, file, line, "", null, null, null) :=
  *edge{{to: {handle}, from: other, kind: kind, file: file, line: line}}.

code_reference(h) :=
  *meta{{handle: h, key: "{external_class}", value: "{class_code}"}}.

code_ref_disposition(h, disposition) :=
  *meta{{handle: h, key: "{referent_disposition}", value: disposition}}.

code_ref_disposition(h, null) :=
  code_reference(h),
  not code_ref_disposition_present(h).

code_ref_disposition_present(h) :=
  *meta{{handle: h, key: "{referent_disposition}", value: disposition}}.

code_ref_candidate_count(h, count) :=
  *meta{{handle: h, key: "{move_candidate_count}", value: count}}.

code_ref_candidate_count(h, null) :=
  code_reference(h),
  not code_ref_candidate_count_present(h).

code_ref_candidate_count_present(h) :=
  *meta{{handle: h, key: "{move_candidate_count}", value: count}}.

code_ref_moved_to(h, target) :=
  *meta{{handle: h, key: "{moved_to}", value: target}}.

code_ref_moved_to(h, null) :=
  code_reference(h),
  not code_ref_moved_to_present(h).

code_ref_moved_to_present(h) :=
  *meta{{handle: h, key: "{moved_to}", value: target}}.

? handle_row(h, relation, other, kind, status, file, line, summary, disposition, candidate_count, moved_to).
"#
    )
}

fn looks_like_retired_section_handle(handle: &str) -> bool {
    handle.contains('#') && !handle.starts_with("http://") && !handle.starts_with("https://")
}

fn retired_section_handle_message(handle: &str) -> String {
    let file = handle.split_once('#').map_or(handle, |(file, _)| file);
    let file_literal = datalog_string_literal(file);
    format!(
        "section handles were retired in v0.14; use `anneal -e '? *span{{handle: {file_literal}, id: span_id, summary: heading}}.'` to find heading spans"
    )
}

fn prelude_error(error: PreludeError) -> anyhow::Error {
    anyhow!(error)
}
