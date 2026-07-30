mod adapter_facade {
    use anneal_core::{
        ConfigFacts, FactBatch, FactBatchMode, FactStore, OneShotSourceDriver, Pattern, Source,
        SourceCapabilities, SourceContext, SourceError, SourceInfo, SourceName,
        SourceRefreshRequest, refresh_source,
    };

    struct FixtureSource;

    impl Source for FixtureSource {
        fn describe(&self) -> SourceInfo {
            SourceInfo {
                name: "fixture",
                recognizes: vec![Pattern::new("**/*.fixture")],
                doc: "third-party adapter facade fixture",
                config_keys: Vec::new(),
                capabilities: SourceCapabilities::default(),
                search: None,
            }
        }

        fn extract(&self, cx: &SourceContext<'_>) -> Result<FactBatch, SourceError> {
            Ok(FactBatch::new(
                cx.corpus.clone(),
                SourceName::from("fixture"),
                FactBatchMode::FullSnapshot,
                cx.next_generation(),
            ))
        }
    }

    #[test]
    fn adapter_contract_is_sufficient_from_the_crate_root() {
        let roots = Vec::new();
        let config = ConfigFacts::default();
        let request = SourceRefreshRequest::new("fixture-corpus", &roots, &config);
        let driver = OneShotSourceDriver::new(FixtureSource);
        let mut store = FactStore::default();

        let report = refresh_source(&driver, &request, &mut store).expect("adapter refreshes");

        assert_eq!(report.source, SourceName::from("fixture"));
        assert_eq!(
            store.generation_for(request.corpus(), &report.source),
            Some(report.current_generation)
        );
    }
}

mod host_facade {
    use anneal_core::runtime::{Database, Evaluator, Value, analyze, parse_program, write_ndjson};

    #[test]
    fn host_contract_is_sufficient_from_the_runtime_facade() {
        let program = parse_program("host-fixture", "answer(\"settled\").\n? answer(value).")
            .expect("program parses");
        let analyzed = analyze(program).expect("program analyzes");
        let query = analyzed.queries().next().expect("fixture query").clone();
        let mut evaluator = Evaluator::new(analyzed, Database::default());

        evaluator.run_fixpoint().expect("program evaluates");
        let output = evaluator.eval_query(&query).expect("query evaluates");
        let mut rendered = Vec::new();
        write_ndjson(&mut rendered, &output.rows).expect("rows render");

        assert_eq!(
            output.rows[0].fields.get("value"),
            Some(&Value::String("settled".to_string()))
        );
        assert_eq!(
            String::from_utf8(rendered).expect("NDJSON is UTF-8"),
            "{\"value\":\"settled\"}\n"
        );
    }
}

mod layered_host {
    use std::cmp::Ordering;

    use anneal_core::runtime::EvalOptions;
    use anneal_core::{ActorContext, Ranker, RankingContext, RuntimeCapability, SearchHit};

    struct FixtureRanker;

    impl Ranker for FixtureRanker {
        fn calibrate(&self, hit: &SearchHit, _context: &RankingContext) -> f32 {
            hit.raw_score().get()
        }

        fn tie_break(&self, left: &SearchHit, right: &SearchHit) -> Ordering {
            left.handle().cmp(right.handle())
        }
    }

    #[test]
    fn configured_host_layers_runtime_over_shared_root_contracts() {
        let actor = ActorContext::anonymous_cli().with_runtime_capability(RuntimeCapability::Eval);
        let options = EvalOptions::default()
            .with_actor(actor)
            .with_ranker(FixtureRanker);

        assert!(
            options
                .actor()
                .has_runtime_capability(RuntimeCapability::Eval)
        );
    }
}
