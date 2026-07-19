#!/usr/bin/env bash

# Shared by the workspace gate and its executable fixtures. Keep opt-in lints
# here as well as in [workspace.lints] so the fixtures exercise the exact policy.
readonly CLIPPY_POLICY_ARGS=(
    -D warnings
    -D clippy::allow_attributes_without_reason
    -D clippy::case_sensitive_file_extension_comparisons
    -D clippy::cast_ptr_alignment
    -D clippy::cfg_not_test
    -D clippy::dbg_macro
    -D clippy::exit
    -D clippy::expect_used
    -D clippy::fallible_impl_from
    -D clippy::fn_to_numeric_cast_any
    -D clippy::large_stack_arrays
    -D clippy::large_types_passed_by_value
    -D clippy::let_underscore_must_use
    -D clippy::non_send_fields_in_send_ty
    -D clippy::panic
    -D clippy::path_buf_push_overwrite
    -D clippy::redundant_clone
    -D clippy::string_slice
    -D clippy::todo
    -D clippy::transmute_ptr_to_ptr
    -D clippy::transmute_undefined_repr
    -D clippy::undocumented_unsafe_blocks
    -D clippy::unimplemented
    -D clippy::unreachable
    -D clippy::unused_result_ok
    -D clippy::unwrap_used
    -D clippy::unwrap_in_result
)
