# Clippy lint inventory

Snapshot: 2026-07-19, Rust/Clippy 1.95.0. Generated with
`./scripts/lint-inventory.sh` over the complete workspace, all targets, and all
features. The table is exhaustive for nonzero findings from `clippy::all`,
`clippy::pedantic`, `clippy::nursery`, and `clippy::restriction`; group lints
with no findings are omitted.

The gate adopts all of `clippy::all` and the explicit safety/reliability list
below. A finding marked “adopted” is not accepted as baseline debt: production
code must be fixed or receive a narrow, reasoned exception. Pedantic and nursery
findings remain measured backlog rather than being enabled wholesale.
Restriction lints are deliberately selected individually because many pairs
encode mutually exclusive styles.

Explicit denies, including those currently at zero: `allow_attributes_without_reason`,
`case_sensitive_file_extension_comparisons`, `cast_ptr_alignment`,
`cfg_not_test`, `dbg_macro`, `exit`, `expect_used`, `fallible_impl_from`,
`fn_to_numeric_cast_any`, `large_stack_arrays`, `large_types_passed_by_value`,
`let_underscore_must_use`, `non_send_fields_in_send_ty`, `panic`,
`path_buf_push_overwrite`, `redundant_clone`, `string_slice`, `todo`,
`transmute_ptr_to_ptr`, `transmute_undefined_repr`,
`undocumented_unsafe_blocks`, `unimplemented`, `unreachable`,
`unused_result_ok`, `unwrap_in_result`, and `unwrap_used`.

## Adversarial promotion queue

A fresh inventory at `41d6c9a` shows that several inconvenient lints are
material production risks, not style rules that can be dismissed by labelling
the whole restriction or pedantic group optional:

| lint | current findings | why it remains high priority |
| --- | ---: | --- |
| `clippy::arithmetic_side_effects` | 465 | Frame, duration, byte-count, and coordinate overflow can change rendered output or panic. Stage this per media/math module with explicit checked, saturating, or justified arithmetic. |
| `clippy::cast_possible_truncation` | 759 | Frame/time and buffer-size conversions need range proofs at their boundaries. |
| `clippy::cast_precision_loss` | 279 | Integer-to-float time and pixel conversions can accumulate visible drift. |
| `clippy::cast_sign_loss` | 182 | Negative timeline values must not silently become large indices or sizes. |
| `clippy::indexing_slicing` | 584 | Decoder buffers and graph collections need checked access at untrusted or computed boundaries. |
| `clippy::map_err_ignore` | 86 | Dropping source errors directly harms the decoder/plugin diagnostics needed for QA. |
| `clippy::mem_forget` | 2 | The plugin ABI currently uses it for ownership transfer; prefer `ManuallyDrop` or document a narrow exception before promotion. |
| `clippy::multiple_unsafe_ops_per_block` | 16 | Plugin/FFI review needs one safety proof per unsafe operation rather than a broad block. |
| `clippy::significant_drop_tightening` | 92 | Long-lived locks or guards can stall render and decode work; verify each finding before enabling the nursery lint. |

These are an explicit remediation queue. Their current nonzero baseline is why
they were not silently promoted by this quality-only change; promotion requires
production fixes and narrow, reasoned exceptions, not a permanent global
allowance.

| lint | findings | groups | decision |
| --- | ---: | --- | --- |
| `clippy::absolute_paths` | 1267 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::allow_attributes` | 60 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::allow_attributes_without_reason` | 42 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::arbitrary_source_item_ordering` | 1750 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::arc_with_non_send_sync` | 4 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::arithmetic_side_effects` | 400 | `restriction` | defer: high-priority media/math audit; promote module by module |
| `clippy::as_conversions` | 1466 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::assertions_on_constants` | 1 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::assertions_on_result_states` | 2 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::assign_op_pattern` | 4 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::assigning_clones` | 32 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::bind_instead_of_map` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::bool_to_int_with_if` | 4 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::branches_sharing_code` | 14 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::case_sensitive_file_extension_comparisons` | 4 | `pedantic` | adopted: explicit deny; fix all production findings |
| `clippy::cast_lossless` | 363 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::cast_possible_truncation` | 702 | `pedantic` | defer: high-priority frame/time/buffer conversion audit |
| `clippy::cast_possible_wrap` | 64 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::cast_precision_loss` | 243 | `pedantic` | defer: high-priority timeline and coordinate conversion audit |
| `clippy::cast_sign_loss` | 166 | `pedantic` | defer: high-priority negative timeline/index boundary audit |
| `clippy::clone_on_copy` | 4 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::clone_on_ref_ptr` | 52 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::cloned_instead_of_copied` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::cloned_ref_to_slice_refs` | 1 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::cognitive_complexity` | 24 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::collapsible_else_if` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::collapsible_if` | 58 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::collapsible_match` | 4 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::create_dir` | 3 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::decimal_literal_representation` | 2 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::default_numeric_fallback` | 643 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::default_trait_access` | 36 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::derivable_impls` | 6 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::derive_partial_eq_without_eq` | 40 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::doc_markdown` | 28 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::doc_paragraphs_missing_punctuation` | 64 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::elidable_lifetime_names` | 20 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::else_if_without_else` | 48 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::enum_variant_names` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::equatable_if_let` | 6 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::exhaustive_enums` | 84 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::exhaustive_structs` | 208 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::expect_used` | 15 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::explicit_iter_loop` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::field_reassign_with_default` | 9 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::field_scoped_visibility_modifiers` | 6 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::float_arithmetic` | 1587 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::float_cmp` | 84 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::format_push_string` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::if_not_else` | 16 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::if_same_then_else` | 6 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::if_then_some_else_none` | 14 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::ignore_without_reason` | 3 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::impl_trait_in_params` | 66 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::implicit_hasher` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::implicit_return` | 5189 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::imprecise_flops` | 14 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::inconsistent_struct_constructor` | 4 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::indexing_slicing` | 392 | `restriction` | defer: high-priority decoder/graph boundary audit; promote by module |
| `clippy::integer_division` | 21 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::integer_division_remainder_used` | 53 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::items_after_statements` | 38 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::iter_over_hash_type` | 59 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::large_enum_variant` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::let_underscore_must_use` | 23 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::let_underscore_untyped` | 31 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::literal_string_with_formatting_args` | 2 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::little_endian_bytes` | 82 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::manual_assert` | 1 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::manual_clamp` | 6 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::manual_is_multiple_of` | 4 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::manual_map` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::manual_midpoint` | 14 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::manual_string_new` | 46 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::manual_strip` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::many_single_char_names` | 20 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::map_err_ignore` | 88 | `restriction` | defer: high-priority diagnostics audit; preserve source error context |
| `clippy::map_unwrap_or` | 84 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::match_same_arms` | 28 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::match_wildcard_for_single_variants` | 14 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::min_ident_chars` | 1142 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::missing_assert_message` | 6 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::missing_asserts_for_indexing` | 18 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::missing_const_for_fn` | 214 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::missing_docs_in_private_items` | 1563 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::missing_errors_doc` | 426 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::missing_inline_in_public_items` | 1656 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::missing_panics_doc` | 22 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::missing_safety_doc` | 14 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::missing_trait_methods` | 313 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::mod_module_files` | 74 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::module_inception` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::module_name_repetitions` | 190 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::modulo_arithmetic` | 36 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::multiple_inherent_impl` | 14 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::multiple_unsafe_ops_per_block` | 16 | `restriction` | defer: high-priority FFI audit; split blocks around individual safety proofs |
| `clippy::must_use_candidate` | 390 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::needless_borrow` | 14 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::needless_borrows_for_generic_args` | 3 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::needless_collect` | 2 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::needless_continue` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::needless_lifetimes` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::needless_pass_by_ref_mut` | 44 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::needless_pass_by_value` | 36 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::needless_raw_strings` | 5 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::needless_update` | 8 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::new_without_default` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::no_effect_underscore_binding` | 12 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::non_ascii_literal` | 32 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::non_std_lazy_statics` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::option_if_let_else` | 104 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::or_fun_call` | 34 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::panic` | 5 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::panic_in_result_fn` | 2 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::partial_pub_fields` | 14 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::pattern_type_mismatch` | 472 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::print_stdout` | 9 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::ptr_arg` | 10 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::pub_use` | 180 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::pub_with_shorthand` | 4 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::question_mark_used` | 779 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::range_plus_one` | 4 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::rc_buffer` | 4 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::redundant_clone` | 12 | `nursery` | adopted: explicit deny; fix all production findings |
| `clippy::redundant_closure` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::redundant_closure_for_method_calls` | 125 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::redundant_else` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::redundant_pub_crate` | 4 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::redundant_test_prefix` | 14 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::redundant_type_annotations` | 10 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::ref_option` | 6 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::ref_patterns` | 17 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::renamed_function_params` | 32 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::return_and_then` | 44 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::return_self_not_must_use` | 6 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::semicolon_if_nothing_returned` | 18 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::semicolon_outside_block` | 15 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::separated_literal_suffix` | 32 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::shadow_reuse` | 571 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::shadow_unrelated` | 253 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::significant_drop_tightening` | 84 | `nursery` | defer: high-priority lock/guard lifetime audit; validate nursery false positives |
| `clippy::similar_names` | 16 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::single_call_fn` | 306 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::single_char_add_str` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::single_char_lifetime_names` | 96 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::single_char_pattern` | 1 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::single_component_path_imports` | 1 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::single_match` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::single_match_else` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::std_instead_of_alloc` | 143 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::std_instead_of_core` | 146 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::str_to_string` | 1228 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::string_slice` | 2 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::struct_excessive_bools` | 10 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::struct_field_names` | 4 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::suboptimal_flops` | 286 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::tests_outside_test_module` | 50 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::too_long_first_doc_paragraph` | 2 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::too_many_arguments` | 40 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::too_many_lines` | 89 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::trivially_copy_pass_by_ref` | 16 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::type_complexity` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::undocumented_unsafe_blocks` | 42 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::uninlined_format_args` | 338 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unnecessary_cast` | 6 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::unnecessary_debug_formatting` | 1 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unnecessary_literal_bound` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unnecessary_semicolon` | 2 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unnecessary_sort_by` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::unnecessary_wraps` | 4 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unneeded_field_pattern` | 8 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::unnested_or_patterns` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unreachable` | 3 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::unreadable_literal` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unseparated_literal_suffix` | 52 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::unused_peekable` | 4 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unused_result_ok` | 4 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::unused_self` | 6 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::unused_trait_names` | 43 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::unwrap_used` | 106 | `restriction` | adopted: explicit deny; fix all production findings |
| `clippy::use_debug` | 4 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::use_self` | 592 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::used_underscore_binding` | 8 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::useless_let_if_seq` | 14 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::while_float` | 4 | `nursery` | defer: measured backlog; promote individually after a zero baseline |
| `clippy::while_let_loop` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::while_let_on_iterator` | 2 | `all` | adopted via `clippy::all`; fix before merge |
| `clippy::wildcard_enum_match_arm` | 128 | `restriction` | exclude globally; restriction lints require case-by-case opt-in |
| `clippy::wildcard_imports` | 3 | `pedantic` | defer: measured backlog; promote individually after a zero baseline |
