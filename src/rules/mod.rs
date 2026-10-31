//! One file per rule. `ALL` is the registry, grouped by area.

use crate::rule::Rule;

mod arc_try_unwrap_clone;
mod borrowed_params;
mod build_rs_hygiene;
mod cargo_feature_hygiene;
mod changelog_format;
mod changelog_required;
mod collection_idioms;
mod compile_everything_on_pr;
mod component_wiring;
mod crate_manifest;
mod dependency_vetting;
mod deterministic_iteration;
mod doc_comment_format;
mod docs_in_sync;
mod docs_index_and_adr;
mod dont_swallow_errors;
mod duplicated_code;
mod error_report_labels;
mod exhaustive_matches;
mod expect_with_reason;
mod explain_magic_values;
mod fml_hygiene;
mod generated_files_untouched;
mod glean_metrics_yaml;
mod http_status_checked;
mod kotlin_gradle_misc;
mod license_header_and_whitespace;
mod lock_once_per_operation;
mod meaningful_assertions;
mod migration_code_frozen;
mod minimal_visibility;
mod naming;
mod needless_clones;
mod no_blocking_io_in_constructors;
mod no_bool_params;
mod no_callbacks_or_io_under_lock;
mod no_commented_out_code;
mod no_debug_leftovers;
mod no_force_unwrap_wrappers;
mod no_global_mutable_statics;
mod no_panicking_index_or_arith;
mod no_per_item_queries;
mod no_pii_in_logs;
mod no_unused_deps;
mod no_unwrap_in_component_code;
mod option_result_combinators;
mod parking_lot_locks;
mod pedantic_subset;
mod persisted_values_stable;
mod pin_ci_tools;
mod pragmas_centralized;
mod preserve_error_sources;
mod python_ruff;
mod remove_dead_pub_code;
mod rs_record_cleanup;
mod rustfmt_config;
mod schema_change_needs_migration;
mod schema_constraints;
mod shell_strict_shellcheck;
mod spellcheck;
mod sql_formatting;
mod sqlite_footguns;
mod swift_access_control;
mod sync_payload_compat;
mod test_isolation;
mod test_structure;
mod tests_with_behaviour;
mod todo_needs_bug;
mod tolerant_remote_deserialization;
mod tracing_support_logging;
mod transaction_must_commit;
mod typed_time_and_units;
mod uniffi_api_break_gate;
mod uniffi_callback_contract;
mod uniffi_docstrings;
mod uniffi_handle_error;
mod uniffi_names_read_well;
mod uniffi_no_panic_across_ffi;
mod uniffi_no_test_only_exports;
mod uniffi_public_errors_actionable;
mod uniffi_record_defaults;
mod uniffi_thin_ffi_layer;
mod uniffi_typed_values;
mod uniffi_udl_proc_macro_consistency;
mod uniffi_unique_exported_names;
mod unsafe_and_nss;
mod url_hygiene;
mod use_sql_support;
mod utf8_safe_truncation;
mod version_sync;
mod wrappers_in_sync;

pub static ALL: &[&Rule] = &[
    &uniffi_api_break_gate::RULE,
    &uniffi_public_errors_actionable::RULE,
    &uniffi_docstrings::RULE,
    &uniffi_record_defaults::RULE,
    &uniffi_typed_values::RULE,
    &uniffi_unique_exported_names::RULE,
    &uniffi_names_read_well::RULE,
    &uniffi_no_panic_across_ffi::RULE,
    &uniffi_thin_ffi_layer::RULE,
    &uniffi_handle_error::RULE,
    &uniffi_no_test_only_exports::RULE,
    &uniffi_callback_contract::RULE,
    &uniffi_udl_proc_macro_consistency::RULE,
    &no_unwrap_in_component_code::RULE,
    &dont_swallow_errors::RULE,
    &error_report_labels::RULE,
    &no_panicking_index_or_arith::RULE,
    &preserve_error_sources::RULE,
    &utf8_safe_truncation::RULE,
    &http_status_checked::RULE,
    &schema_change_needs_migration::RULE,
    &schema_constraints::RULE,
    &use_sql_support::RULE,
    &no_per_item_queries::RULE,
    &sqlite_footguns::RULE,
    &migration_code_frozen::RULE,
    &sql_formatting::RULE,
    &pragmas_centralized::RULE,
    &transaction_must_commit::RULE,
    &rs_record_cleanup::RULE,
    &persisted_values_stable::RULE,
    &tolerant_remote_deserialization::RULE,
    &sync_payload_compat::RULE,
    &no_callbacks_or_io_under_lock::RULE,
    &lock_once_per_operation::RULE,
    &parking_lot_locks::RULE,
    &no_blocking_io_in_constructors::RULE,
    &no_global_mutable_statics::RULE,
    &arc_try_unwrap_clone::RULE,
    &no_pii_in_logs::RULE,
    &tracing_support_logging::RULE,
    &spellcheck::RULE,
    &license_header_and_whitespace::RULE,
    &docs_in_sync::RULE,
    &changelog_format::RULE,
    &explain_magic_values::RULE,
    &changelog_required::RULE,
    &todo_needs_bug::RULE,
    &doc_comment_format::RULE,
    &docs_index_and_adr::RULE,
    &remove_dead_pub_code::RULE,
    &expect_with_reason::RULE,
    &no_unused_deps::RULE,
    &no_commented_out_code::RULE,
    &no_debug_leftovers::RULE,
    &option_result_combinators::RULE,
    &needless_clones::RULE,
    &pedantic_subset::RULE,
    &rustfmt_config::RULE,
    &collection_idioms::RULE,
    &borrowed_params::RULE,
    &exhaustive_matches::RULE,
    &unsafe_and_nss::RULE,
    &duplicated_code::RULE,
    &deterministic_iteration::RULE,
    &naming::RULE,
    &minimal_visibility::RULE,
    &url_hygiene::RULE,
    &no_bool_params::RULE,
    &typed_time_and_units::RULE,
    &dependency_vetting::RULE,
    &compile_everything_on_pr::RULE,
    &component_wiring::RULE,
    &cargo_feature_hygiene::RULE,
    &version_sync::RULE,
    &generated_files_untouched::RULE,
    &crate_manifest::RULE,
    &build_rs_hygiene::RULE,
    &test_isolation::RULE,
    &tests_with_behaviour::RULE,
    &test_structure::RULE,
    &meaningful_assertions::RULE,
    &shell_strict_shellcheck::RULE,
    &pin_ci_tools::RULE,
    &python_ruff::RULE,
    &kotlin_gradle_misc::RULE,
    &no_force_unwrap_wrappers::RULE,
    &swift_access_control::RULE,
    &wrappers_in_sync::RULE,
    &glean_metrics_yaml::RULE,
    &fml_hygiene::RULE,
];
