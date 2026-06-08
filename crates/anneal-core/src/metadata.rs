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

    pub const CLASS_CODE: &str = "code";
}
