//! Metadata markers for code-target probes.

pub struct CodeTargetMeta;

impl CodeTargetMeta {
    pub const EXTERNAL_CLASS: &str = code_target::EXTERNAL_CLASS;
    pub const TARGET_PATH: &str = code_target::TARGET_PATH;
    pub const TARGET_START_LINE: &str = code_target::TARGET_START_LINE;
    pub const TARGET_END_LINE: &str = code_target::TARGET_END_LINE;
    pub const TARGET_EXISTS: &str = code_target::TARGET_EXISTS;
    pub const TARGET_HISTORY_STATUS: &str = code_target::TARGET_HISTORY_STATUS;
    pub const TARGET_PROBE_BASE: &str = code_target::TARGET_PROBE_BASE;
    pub const TARGET_RESOLVED_PATH: &str = code_target::TARGET_RESOLVED_PATH;
    pub const REFERENT_DISPOSITION: &str = code_target::REFERENT_DISPOSITION;
    pub const REFERENT_COMMITS_SINCE: &str = code_target::REFERENT_COMMITS_SINCE;
    pub const REFERENT_MOVED_TO: &str = code_target::REFERENT_MOVED_TO;
    pub const REFERENT_MOVE_CANDIDATE: &str = code_target::REFERENT_MOVE_CANDIDATE;
    pub const REFERENT_MOVE_CANDIDATE_COUNT: &str = code_target::REFERENT_MOVE_CANDIDATE_COUNT;
    pub const REFERENT_EVIDENCE_HEAD: &str = code_target::REFERENT_EVIDENCE_HEAD;
    pub const REFERENT_ASSERTION_PREMISE: &str = code_target::REFERENT_ASSERTION_PREMISE;

    pub const CLASS_CODE: &str = code_target::CLASS_CODE;
}

pub mod code_target {
    pub const EXTERNAL_CLASS: &str = "external_class";
    pub const TARGET_PATH: &str = "target_path";
    pub const TARGET_START_LINE: &str = "target_start_line";
    pub const TARGET_END_LINE: &str = "target_end_line";
    pub const TARGET_EXISTS: &str = "target_exists";
    pub const TARGET_HISTORY_STATUS: &str = "target_history_status";
    pub const TARGET_PROBE_BASE: &str = "target_probe_base";
    pub const TARGET_RESOLVED_PATH: &str = "target_resolved_path";
    pub const REFERENT_DISPOSITION: &str = "code.referent_disposition";
    pub const REFERENT_COMMITS_SINCE: &str = "code.referent_commits_since";
    pub const REFERENT_MOVED_TO: &str = "code.referent_moved_to";
    pub const REFERENT_MOVE_CANDIDATE: &str = "code.referent_move_candidate";
    pub const REFERENT_MOVE_CANDIDATE_COUNT: &str = "code.referent_move_candidate_count";
    pub const REFERENT_EVIDENCE_HEAD: &str = "code.referent_evidence_head";
    pub const REFERENT_ASSERTION_PREMISE: &str = "code.referent_assertion_premise";

    pub const CLASS_CODE: &str = "code";
}
