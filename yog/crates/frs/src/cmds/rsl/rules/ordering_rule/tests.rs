use std::path::Path;
use std::path::PathBuf;

use item_source_ranges::SourcePosition;

use super::*;
use crate::cmds::rsl::output::FormattedRuleViolation;
use crate::cmds::rsl::output::ViolationOutputFormat;

#[rstest::rstest]
#[case(vec![0, 1], Vec::new(), Ok(()))]
#[case(vec![0], Vec::new(), Err("incomplete item permutation"))]
#[case(vec![0, 0], Vec::new(), Err("incomplete item permutation"))]
#[case(
    vec![0, 1],
    vec![OrderingConstraint { before_item_index: 0, after_item_index: 1, kind: OrderingConstraintKind::CallerBeforeHelper }],
    Ok(())
)]
#[case(
    vec![1, 0],
    vec![OrderingConstraint { before_item_index: 0, after_item_index: 1, kind: OrderingConstraintKind::CallerBeforeHelper }],
    Err("invalid item identity or ordering constraint")
)]
fn test_ordering_rule_when_required_item_order_is_validated_reports_invalid_item_order(
    #[case] required_item_order: Vec<usize>,
    #[case] constraints: Vec<OrderingConstraint>,
    #[case] expected: Result<(), &'static str>,
) {
    assert_eq!(
        validate_required_item_order(&[0, 1], &required_item_order, &constraints),
        expected
    );
}

#[rstest::rstest]
#[case(
    r"
        pub macro m() {}
    ",
    Some(ItemKind::Macro)
)]
#[case(
    r"
        #[cfg(test)]
        pub(crate) macro m {}
    ",
    Some(ItemKind::Macro)
)]
#[case(
    r"
        macro m;
    ",
    None
)]
#[case(
    r"
        fn run() {}
    ",
    None
)]
fn test_ordering_rule_when_verbatim_tokens_are_classified_recognizes_only_macro_definitions(
    #[case] source: &str,
    #[case] expected: Option<ItemKind>,
) {
    let tokens = source.parse().unwrap();
    assert_eq!(ast::classify_item(&Item::Verbatim(tokens)), expected);
}

#[rstest::rstest]
#[case(
    r"
        use std::fmt;
        mod child {}
        #[cfg(test)]
        mod tests {}
        const VALUE: () = ();
        struct Data;
        impl Data {}
        fn run() {}
    ",
    vec![0, 1, 2, 3, 4, 5, 6]
)]
#[case(
    r"
        use std::fmt;
        struct Data;
        impl Data {}
        fn run() {}
        #[cfg(test)]
        mod tests {}
        mod child {}
        const VALUE: () = ();
    ",
    vec![0, 5, 4, 6, 1, 2, 3]
)]
#[case(
    r"
        use std::fmt;
        #[cfg(test)]
        pub mod tests {}
        mod private {}
        pub struct Data;
    ",
    vec![0, 2, 1, 3]
)]
fn test_ordering_rule_when_test_module_is_present_keeps_it_last_within_module_group(
    #[case] source: &str,
    #[case] expected: Vec<usize>,
) {
    let violations = check(source);
    let item_count = syn::parse_file(source).unwrap().items.len();
    let actual = violations
        .first()
        .map_or_else(|| (0..item_count).collect(), required_item_order);
    assert_eq!(actual, expected);
    let reordered = apply_ordering_violations(source, &violations);
    assert_eq!(check(&reordered), Vec::new());
    assert_eq!(check(source), violations);
}

#[test]
fn test_ordering_rule_when_nested_test_module_follows_fn_moves_it_into_module_group() {
    let source = r"
        mod parent {
            use std::fmt;
            mod child {}
            fn run() {}
            #[cfg(test)]
            mod tests {}
        }
    ";
    let violations = check(source);
    assert_eq!(violations.len(), 1);
    assert_eq!(required_item_order(&violations[0]), vec![0, 1, 3, 2]);
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[rstest::rstest]
#[case(
    r"
        use std::path::Path;

        use external::Arguments;

        pub use self::output::Output;
        use self::output::Format;
    "
)]
#[case(
    r"
        mod child {
            use std::path::Path;

            use external::Arguments;

            pub use self::output::Output;
            use self::output::Format;
        }
    "
)]
fn test_ordering_rule_when_imports_follow_formatter_order_preserves_visibility_mix(#[case] source: &str) {
    assert_eq!(check(source), Vec::new());
}

#[test]
fn test_ordering_rule_when_import_follows_fn_moves_import_without_changing_import_order() {
    let source = r"
        use std::path::Path;
        fn helper() {}
        pub use self::output::Output;
        pub fn caller() {
            helper();
        }
    ";
    let violations = check(source);
    assert_eq!(required_item_order(&violations[0]), vec![0, 2, 3, 1]);
    let reordered = apply_ordering_violations(source, &violations);
    assert_eq!(check(&reordered), Vec::new());
    let expected = r"
        use std::path::Path;
        pub use self::output::Output;
        pub fn caller() {
            helper();
        }
        fn helper() {}
    ";
    assert_eq!(reordered, expected);
}

#[rstest::rstest]
#[case(
    r"
        const VALUE: () = ();
        macro_rules! m {
            () => {};
        }
        mod child {
            m!();
        }
    ",
    vec![0, 1, 2]
)]
#[case(
    r"
        mod child {
            m!();
        }
        macro_rules! m {
            () => {};
        }
        const VALUE: () = ();
    ",
    vec![2, 1, 0]
)]
#[case(
    r#"
        #[cfg(feature = "first")]
        struct Data;
        #[cfg(not(feature = "first"))]
        struct Data;
        fn unrelated() {}
        impl Data {}
    "#,
    vec![0, 1, 3, 2]
)]
fn test_ordering_rule_when_agreed_exceptions_apply_returns_required_sequence(
    #[case] source: &str,
    #[case] expected: Vec<usize>,
) {
    let violations = check(source);
    let syntax = syn::parse_file(source).unwrap();
    let actual = violations.first().map_or_else(
        || (0..syntax.items.len()).collect(),
        |violation| match &violation.outcome {
            OrderingOutcome::Computed {
                required_item_order, ..
            } => required_item_order.clone(),
            OrderingOutcome::Failed { message, .. } => panic!("{message}"),
        },
    );
    assert_eq!(actual, expected);
    let reordered = apply_ordering_violations(source, &violations);
    assert_eq!(check(&reordered), Vec::new(), "{reordered}");
    assert_eq!(check(source), violations);
}

#[test]
fn test_ordering_rule_when_private_helper_precedes_caller_retains_complete_metadata_and_renders_only_order() {
    let source = single_line(
        r"
            fn helper() {}
            fn caller() {
                helper();
            }
        ",
    );
    let actual = check(&source);
    let expected = vec![OrderingRuleViolation {
        location: Location::new(PathBuf::from("test.rs"), 1, 1),
        scope: "module crate".to_owned(),
        depth: 0,
        items: vec![
            ItemOrderingMetadata {
                range: SourceRange {
                    start: SourcePosition { line: 1, column: 1 },
                    end: SourcePosition { line: 1, column: 15 },
                    byte_range: 0..14,
                    whole_lines: false,
                },
                label: "fn helper".to_owned(),
                kind: Some(ItemKind::Fn),
                group: ItemGroup::Items,
                visibility: Some(VisibilityClass::Private),
                block_first_item_index: 0,
            },
            ItemOrderingMetadata {
                range: SourceRange {
                    start: SourcePosition { line: 1, column: 16 },
                    end: SourcePosition { line: 1, column: 41 },
                    byte_range: 15..40,
                    whole_lines: false,
                },
                label: "fn caller".to_owned(),
                kind: Some(ItemKind::Fn),
                group: ItemGroup::Items,
                visibility: Some(VisibilityClass::Private),
                block_first_item_index: 1,
            },
        ],
        outcome: OrderingOutcome::Computed {
            source_item_order: vec![0, 1],
            required_item_order: vec![1, 0],
            constraints: vec![OrderingConstraint {
                before_item_index: 1,
                after_item_index: 0,
                kind: OrderingConstraintKind::CallerBeforeHelper,
            }],
        },
    }];
    assert_eq!(actual, expected);
    assert_eq!(
        actual[0].format(ViolationOutputFormat::Compact),
        "test.rs:1:1,module crate: order=[1:16-1:41,1:1-1:15]"
    );
    assert_eq!(
        actual[0].format(ViolationOutputFormat::Debug),
        "test.rs:1:1,ordering_rule,module crate: order=[1:16-1:41,1:1-1:15]"
    );
}

#[test]
fn test_ordering_rule_when_type_and_impl_are_separated_retains_complete_block_identity() {
    let source = r"
        struct Data;
        fn unrelated() {}
        impl Data {}
    ";
    let actual = check(source);
    assert_eq!(
        actual,
        vec![OrderingRuleViolation {
            location: Location::new(PathBuf::from("test.rs"), 1, 1),
            scope: "module crate".to_owned(),
            depth: 0,
            items: vec![
                ItemOrderingMetadata {
                    range: SourceRange {
                        start: SourcePosition { line: 2, column: 1 },
                        end: SourcePosition { line: 2, column: 21 },
                        byte_range: 1..21,
                        whole_lines: true
                    },
                    label: "struct Data".to_owned(),
                    kind: Some(ItemKind::Struct),
                    group: ItemGroup::Items,
                    visibility: Some(VisibilityClass::Private),
                    block_first_item_index: 0,
                },
                ItemOrderingMetadata {
                    range: SourceRange {
                        start: SourcePosition { line: 3, column: 1 },
                        end: SourcePosition { line: 3, column: 26 },
                        byte_range: 22..47,
                        whole_lines: true
                    },
                    label: "fn unrelated".to_owned(),
                    kind: Some(ItemKind::Fn),
                    group: ItemGroup::Items,
                    visibility: Some(VisibilityClass::Private),
                    block_first_item_index: 1,
                },
                ItemOrderingMetadata {
                    range: SourceRange {
                        start: SourcePosition { line: 4, column: 1 },
                        end: SourcePosition { line: 4, column: 21 },
                        byte_range: 48..68,
                        whole_lines: true
                    },
                    label: "impl Data".to_owned(),
                    kind: Some(ItemKind::Impl),
                    group: ItemGroup::Items,
                    visibility: None,
                    block_first_item_index: 0,
                },
            ],
            outcome: OrderingOutcome::Computed {
                source_item_order: vec![0, 1, 2],
                required_item_order: vec![0, 2, 1],
                constraints: vec![OrderingConstraint {
                    before_item_index: 0,
                    after_item_index: 2,
                    kind: OrderingConstraintKind::TypeImplAdjacency
                }],
            },
        }]
    );
}

#[rstest::rstest]
#[case(
    r"
        fn helper() {}
        fn caller() {
            self::helper();
        }
    "
)]
#[case(
    r"
        fn helper() {}
        #[case(helper())]
        fn caller() {}
    "
)]
#[case(
    r"
        #[test]
        fn helper() {}
        #[rstest::case(helper())]
        fn caller() {}
    "
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            assert!(helper());
        }
    "
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            custom!(nested => [self::helper()]);
        }
    "
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            stringify!(helper());
        }
    "
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            custom!(helper);
        }
    "
)]
fn test_ordering_rule_when_syntactic_reference_calls_helper_moves_it_after_caller(#[case] source: &str) {
    let violations = check(source);
    assert_eq!(required_item_order(&violations[0]), vec![1, 0]);
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[rstest::rstest]
#[case(
    r"
        fn helper() {}
        fn caller() {
            helper!();
        }
    "
)]
#[case(
    r#"
        fn helper() {}
        fn caller() {
            custom!("helper()");
        }
    "#
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            custom!($helper, foreign::helper(), ::helper(), self::nested::helper());
        }
    "
)]
#[case(
    r"
        fn helper() {}
        fn caller() {
            fn nested() {
                helper();
            }
        }
    "
)]
#[case(
    r"
        pub fn first() {
            helper();
        }
        fn helper() {}
        fn late() {
            helper();
        }
    "
)]
#[case(
    r"
        fn first() {
            custom!(helper);
        }
        fn helper() {}
        fn late() {
            helper();
        }
    "
)]
fn test_ordering_rule_when_no_unsatisfied_relationship_exists_preserves_source_order(#[case] source: &str) {
    assert_eq!(check(source), Vec::new());
}

#[rstest::rstest]
#[case(
    r"
        fn helper() {}
        fn caller() {
            helper();
        }
        fn tail() {
            caller();
        }
    "
)]
#[case(
    r"
        fn a() {
            b();
        }
        fn unrelated() {}
        fn b() {
            a();
        }
        pub fn entry() {
            a();
        }
    "
)]
#[case(
    r"
        fn private() {
            helper();
        }
        fn helper() {}
        pub fn public() {
            helper();
        }
    "
)]
#[case(
    r"
        fn helper() {}
        struct Data;
        fn unrelated() {}
        impl Data {
            fn draw() {
                helper();
            }
        }
    "
)]
#[case(
    r"
        impl Data {
            fn helper() {}
            pub fn entry() {
                Self::helper();
            }
        }
        fn unrelated() {}
        pub struct Data;
    "
)]
#[case(
    r"
        impl Data {
            fn helper() {}
            fn entry() {
                Self::helper();
            }
        }
        struct Data;
    "
)]
#[case(
    r"
        impl Trait for Data {}
        const VALUE: () = ();
        struct Data;
        fn unrelated() {}
        impl Data {}
        trait Trait {}
    "
)]
#[case(
    r"
        #[cfg(test)]
        mod tests {}
        fn run() {}
        pub(crate) const VALUE: () = ();
        pub const PUBLIC: () = ();
        mod child {}
        use std::fmt;
        extern crate core;
    "
)]
#[case(
    r"
        opaque!();
        fn private() {}
        pub fn public() {}
    "
)]
#[case(
    r"
        mod parent {
            impl Data {
                fn helper() {}
                pub fn entry() {
                    Self::helper();
                }
            }
            fn private() {}
            pub struct Data;
        }
        pub const VALUE: () = ();
    "
)]
fn test_ordering_rule_when_constraints_interact_produces_repeatable_order_and_passes_after_application(
    #[case] source: &str,
) {
    let violations = check(source);
    assert_ne!(violations, Vec::new());
    assert_eq!(check(source), violations);
    let actual = apply_ordering_violations(source, &violations);
    assert_eq!(check(&actual), Vec::new(), "{actual}");
    // Parsing after application also proves item boundaries remain syntactically intact.
    syn::parse_file(&actual).unwrap();
}

#[test]
fn test_ordering_rule_when_scopes_are_nested_emits_deepest_first_and_preserves_snapshot_identity() {
    let source = r"
        fn private() {}
        pub mod child {
            fn private() {}
            pub struct Data;
            impl Data {
                fn private() {}
                pub fn public() {}
            }
        }
    ";
    let violations = check(source);
    let actual: Vec<_> = violations
        .iter()
        .map(|violation| (violation.depth, violation.scope.as_str()))
        .collect();
    assert_eq!(
        actual,
        vec![(2, "impl Data"), (1, "module crate::child"), (0, "module crate")]
    );
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[test]
fn test_ordering_rule_when_items_have_comments_keeps_attached_content_and_scope_trivia() {
    let source = r"
        //! scope docs

        // scope header

        // private note
        #[cfg(unix)]
        fn private() {} // trailing private
        /* public note */
        /// public docs
        pub fn public() {}
    ";
    let violations = check(source);
    let actual = apply_ordering_violations(source, &violations);
    let expected = r"
        //! scope docs

        // scope header

        /* public note */
        /// public docs
        pub fn public() {}
        // private note
        #[cfg(unix)]
        fn private() {} // trailing private
    ";
    assert_eq!(actual, expected);
    assert_eq!(check(&actual), Vec::new());
    assert_eq!(
        violations[0]
            .items
            .iter()
            .map(|item| item.range.format_compact())
            .collect::<Vec<_>>(),
        vec!["6-8", "9-11"]
    );
}

#[rstest::rstest]
#[case(
    r"
        mod child {
            macro_rules! m {
                () => {};
            }
            m!();
        }
        macro_rules! m {
            () => {};
        }
    ",
    false
)]
#[case(
    r"
        macro_rules! m {
            () => {};
        }
        pub mod first {
            m!();
        }
        macro_rules! m {
            () => {};
        }
        pub mod second {
            m!();
        }
    ",
    false
)]
#[case(
    r"
        mod child {
            fn run() {
                macro_rules! m {
                    () => {};
                }
                m!();
            }
        }
        macro_rules! m {
            () => {};
        }
    ",
    false
)]
#[case(
    r"
        mod child {
            m!();
            macro_rules! m {
                () => {};
            }
            m!();
        }
        macro_rules! m {
            () => {};
        }
    ",
    true
)]
#[case(
    r"
        mod child {
            mod shadowed {
                macro_rules! m {
                    () => {};
                }
                m!();
            }
            mod unshadowed {
                m!();
            }
        }
        macro_rules! m {
            () => {};
        }
    ",
    true
)]
fn test_ordering_rule_when_inner_macro_shadows_outer_respects_lexical_scope(
    #[case] source: &str,
    #[case] requires_outer: bool,
) {
    let violations = check(source);
    assert_eq!(!violations.is_empty(), requires_outer);
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[rstest::rstest]
#[case(
    r"
        // private note
        fn private() {}
        pub fn public() {}
    ",
    r"
        pub fn public() {}
        // private note
        fn private() {}
    "
)]
#[case(
    r"
        fn private() {} /* trailing
                          /* nested */
                          */
        pub fn public() {}
    ",
    r"
        pub fn public() {}
        fn private() {} /* trailing
                          /* nested */
                          */
    "
)]
#[case(
    r"
        fn private() {} /* first */ // private tail
        pub fn public() {}
    ",
    r"
        pub fn public() {}
        fn private() {} /* first */ // private tail
    "
)]
#[case(
    r"
        fn private() {}
        // scope header

        pub fn public() {}
    ",
    r"
        pub fn public() {}
        // scope header

        fn private() {}
    "
)]
#[case(
    r"
        // private note */
        fn private() {}
        pub fn public() {}
    ",
    r"
        pub fn public() {}
        // private note */
        fn private() {}
    "
)]
fn test_ordering_rule_when_ranges_include_trivia_preserves_complete_items(
    #[case] source: &str,
    #[case] expected: &str,
) {
    let actual = apply_ordering_violations(source, &check(source));
    assert_eq!(actual, expected);
    assert_eq!(check(&actual), Vec::new());
}

#[rstest::rstest]
#[case(
    r"
        fn α() {}
        pub fn β() {}
    ",
    r"
        pub fn β() {}
        fn α() {}
    "
)]
#[case(
    r"
        mod child {
            /* private note */
            fn private() {}
            pub fn public() {}
        }
    ",
    r"
        mod child {
            pub fn public() {}
            /* private note */
            fn private() {}
        }
    "
)]
#[case(
    r"
        mod child {
            #![allow(dead_code)]
            /* scope note */
            fn private() {}
            pub fn public() {}
        }
    ",
    r"
        mod child {
            #![allow(dead_code)]
            /* scope note */
            pub fn public() {}
            fn private() {}
        }
    "
)]
fn test_ordering_rule_when_items_share_line_preserves_precise_boundaries_and_unicode(
    #[case] source: &str,
    #[case] expected: &str,
) {
    // Keep fixtures readable above; precise-boundary coverage requires same-line source at runtime.
    let source = single_line(source);
    let expected = single_line(expected);
    let actual = apply_ordering_violations(&source, &check(&source));
    assert_eq!(actual, expected);
    assert_eq!(check(&actual), Vec::new());
}

#[rstest::rstest]
#[case(
    "macro_deferred",
    r"
        pub mod child {
            m!();
        }
        macro_rules! m {
            () => {
                const _: () = super::VALUE;
            };
        }
        const VALUE: () = ();
    ",
    r"
        const VALUE: () = ();
        macro_rules! m {
            () => {
                const _: () = super::VALUE;
            };
        }
        pub mod child {
            m!();
        }
    "
)]
#[case(
    "conditional_types",
    r#"
        #[cfg(feature = "first")]
        pub struct Data;
        #[cfg(not(feature = "first"))]
        pub struct Data;
        pub fn entry() -> Data {
            Data::new()
        }
        impl Data {
            pub fn new() -> Self {
                Self
            }
        }
    "#,
    r#"
        #[cfg(feature = "first")]
        pub struct Data;
        #[cfg(not(feature = "first"))]
        pub struct Data;
        impl Data {
            pub fn new() -> Self {
                Self
            }
        }
        pub fn entry() -> Data {
            Data::new()
        }
    "#
)]
#[case(
    "recursive_types",
    r"
        fn odd(n: u32) -> bool {
            n != 0 && even(n - 1)
        }
        pub fn parity(n: u32) -> bool {
            even(n)
        }
        pub struct Data {
            pub value: u32,
        }
        impl Data {
            pub fn new(value: u32) -> Self {
                Self { value }
            }
        }
        fn even(n: u32) -> bool {
            n == 0 || odd(n - 1)
        }
    ",
    r"
        pub fn parity(n: u32) -> bool {
            even(n)
        }
        pub struct Data {
            pub value: u32,
        }
        impl Data {
            pub fn new(value: u32) -> Self {
                Self { value }
            }
        }
        fn odd(n: u32) -> bool {
            n != 0 && even(n - 1)
        }
        fn even(n: u32) -> bool {
            n == 0 || odd(n - 1)
        }
    "
)]
#[case(
    "macro_shadowing",
    r"
        pub mod child {
            macro_rules! m {
                () => {
                    pub const INNER: () = ();
                };
            }
            m!();
        }
        macro_rules! m {
            () => {
                pub const OUTER: () = ();
            };
        }
        pub mod sibling {
            m!();
        }
    ",
    r"
        pub mod child {
            macro_rules! m {
                () => {
                    pub const INNER: () = ();
                };
            }
            m!();
        }
        macro_rules! m {
            () => {
                pub const OUTER: () = ();
            };
        }
        pub mod sibling {
            m!();
        }
    "
)]
fn test_ordering_rule_when_compilable_fixture_is_reordered_matches_verified_source(
    #[case] name: &str,
    #[case] source: &str,
    #[case] expected: &str,
) {
    let actual = apply_ordering_violations(source, &check(source));
    assert_eq!(actual, expected, "{name}");
    assert_eq!(check(&actual), Vec::new(), "{name}");
}

#[test]
fn test_ordering_rule_when_scope_labels_repeat_identifies_each_scope_by_original_position() {
    let source = r"
        #[cfg(unix)]
        impl Data {
            fn private() {}
            pub fn public() {}
        }
        #[cfg(windows)]
        impl Data {
            fn private() {}
            pub fn public() {}
        }
    ";
    let violations = check(source);
    let actual: Vec<_> = violations
        .iter()
        .map(|violation| violation.format(ViolationOutputFormat::Debug))
        .collect();
    assert_eq!(
        actual,
        vec![
            "test.rs:3:9,ordering_rule,impl Data: order=[5-5,4-4]",
            "test.rs:8:9,ordering_rule,impl Data: order=[10-10,9-9]",
        ]
    );
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[rstest::rstest]
#[case("tests.rs", vec![1, 0])]
#[case("test_parser.rs", vec![1, 0])]
#[case("parser_test.rs", vec![1, 0])]
#[case("parser_tests.rs", vec![1, 0])]
#[case("latest.rs", vec![0, 1])]
#[case("test.rs", vec![0, 1])]
#[case("src/tests.rs", vec![1, 0])]
#[case("test_parser.txt", vec![0, 1])]
fn test_ordering_rule_when_filename_selects_test_scope_orders_fn_sections(
    #[case] path: &str,
    #[case] expected: Vec<usize>,
) {
    let source = r"
        pub fn helper() {}
        #[test]
        fn example() {
            helper();
        }
    ";
    let violations = check_source_at_path(source, path);
    assert_eq!(root_item_order(source, &violations), expected);
    assert_eq!(
        check_source_at_path(&apply_ordering_violations(source, &violations), path),
        Vec::new()
    );
    assert_eq!(check_source_at_path(source, path), violations);
}

#[rstest::rstest]
#[case(
    r"
        #[test]
        fn example() {}
        fn helper() {}
    ",
    vec![1, 0]
)]
#[case(
    r"
        #[tokio::test]
        fn example() {}
        fn helper() {}
    ",
    vec![1, 0]
)]
#[case(
    r"
        #[rstest::rstest]
        fn example() {}
        fn helper() {}
    ",
    vec![1, 0]
)]
#[case(
    r"
        #[custom_test]
        fn example() {}
        fn helper() {}
    ",
    vec![1, 0]
)]
#[case(
    r"
        #[test_case(1)]
        fn example() {}
        fn helper() {}
    ",
    vec![1, 0]
)]
#[case(
    r"
        #[latest]
        fn example() {}
        fn helper() {}
    ",
    vec![0, 1]
)]
#[case(
    r"
        #[cfg(test)]
        fn example() {}
        fn helper() {}
    ",
    vec![0, 1]
)]
fn test_ordering_rule_when_attribute_path_marks_tests_ignores_attribute_arguments(
    #[case] source: &str,
    #[case] expected: Vec<usize>,
) {
    let violations = check(source);
    assert_eq!(root_item_order(source, &violations), expected);
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[rstest::rstest]
#[case("tests.rs", vec![1, 0, 2])]
#[case("lib.rs", vec![0, 2, 1])]
fn test_ordering_rule_when_recursion_crosses_sections_keeps_sections_separate(
    #[case] path: &str,
    #[case] expected: Vec<usize>,
) {
    let source = r"
        fn helper() {
            example();
        }
        #[test]
        fn example() {
            helper();
        }
        fn unrelated() {}
    ";
    let violations = check_source_at_path(source, path);
    assert_eq!(root_item_order(source, &violations), expected);
    assert_eq!(
        check_source_at_path(&apply_ordering_violations(source, &violations), path),
        Vec::new()
    );
}

#[test]
fn test_ordering_rule_when_test_module_contains_nested_scopes_inherits_helper_section() {
    let source = r"
        #[cfg(test)]
        mod verification {
            mod nested {
                struct Data;
                impl Data {
                    pub fn helper() {}
                    #[test]
                    fn example() {}
                }
                fn helper() {}
                #[test]
                fn example() {}
            }
        }
    ";
    let violations = check(source);
    assert_eq!(violations.len(), 2);
    assert_eq!(required_item_order(&violations[0]), vec![1, 0]);
    assert_eq!(required_item_order(&violations[1]), vec![0, 1, 3, 2]);
    assert_eq!(check(&apply_ordering_violations(source, &violations)), Vec::new());
}

#[test]
fn test_ordering_rule_when_helpers_share_test_section_retains_caller_and_recursive_order() {
    let source = r"
        fn second() {
            first();
        }
        fn unrelated() {}
        fn first() {
            second();
        }
        pub fn caller() {
            first();
        }
        #[test]
        fn example() {
            first();
        }
    ";
    let violations = check_source_at_path(source, "tests.rs");
    assert_eq!(root_item_order(source, &violations), vec![4, 3, 0, 2, 1]);
    assert_eq!(
        check_source_at_path(&apply_ordering_violations(source, &violations), "tests.rs"),
        Vec::new()
    );
}

#[rstest::rstest]
#[case("lib.rs", vec![1, 0])]
#[case("tests.rs", vec![0, 1])]
fn test_ordering_rule_when_impl_scope_contains_tests_applies_file_section_order(
    #[case] path: &str,
    #[case] expected: Vec<usize>,
) {
    let source = r"
        struct Data;
        impl Data {
            #[test]
            pub fn example() {}
            fn helper() {}
        }
    ";
    let violations = check_source_at_path(source, path);
    let item_order = violations.first().map_or_else(|| vec![0, 1], required_item_order);
    assert_eq!(item_order, expected);
    assert_eq!(
        check_source_at_path(&apply_ordering_violations(source, &violations), path),
        Vec::new()
    );
}

#[test]
fn test_ordering_rule_when_test_fns_recurse_retains_their_adjacent_block() {
    let source = r"
        #[test]
        fn first() {
            second();
        }
        fn helper() {}
        #[test]
        fn second() {
            first();
        }
    ";
    let violations = check_source_at_path(source, "tests.rs");
    assert_eq!(root_item_order(source, &violations), vec![0, 2, 1]);
    assert_eq!(
        check_source_at_path(&apply_ordering_violations(source, &violations), "tests.rs"),
        Vec::new()
    );
}

fn check(source: &str) -> Vec<OrderingRuleViolation> {
    check_source_at_path(source, "test.rs")
}

fn check_source_at_path(source: &str, path: &str) -> Vec<OrderingRuleViolation> {
    let syntax = syn::parse_file(source).unwrap();
    OrderingRule.check(&FileContext {
        path: Path::new(path),
        source,
        file: &syntax,
    })
}

fn root_item_order(source: &str, violations: &[OrderingRuleViolation]) -> Vec<usize> {
    violations
        .iter()
        .find(|violation| violation.scope == "module crate")
        .map_or_else(
            || (0..syn::parse_file(source).unwrap().items.len()).collect(),
            required_item_order,
        )
}

fn single_line(source: &str) -> String {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn apply_ordering_violations(source: &str, violations: &[OrderingRuleViolation]) -> String {
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for violation in violations {
        let source_item_ranges: Vec<_> = violation
            .items
            .iter()
            .map(|item| item.range.byte_range.clone())
            .collect();
        let mut replacement = String::new();
        for (slot, index) in required_item_order(violation).into_iter().enumerate() {
            let item = &source_item_ranges[index];
            replacement.push_str(&render_range_with_edits(source, item.clone(), &edits));
            if let Some(next) = source_item_ranges.get(slot.saturating_add(1)) {
                replacement.push_str(source.get(source_item_ranges[slot].end..next.start).unwrap());
            }
        }
        let range = source_item_ranges.first().unwrap().start..source_item_ranges.last().unwrap().end;
        edits.retain(|(nested, _)| !(nested.start >= range.start && nested.end <= range.end));
        edits.push((range, replacement));
        edits.sort_by_key(|(range, _)| range.start);
    }
    render_range_with_edits(source, 0..source.len(), &edits)
}

fn required_item_order(violation: &OrderingRuleViolation) -> Vec<usize> {
    match &violation.outcome {
        OrderingOutcome::Computed {
            required_item_order, ..
        } => required_item_order.clone(),
        OrderingOutcome::Failed { message, .. } => panic!("{message}"),
    }
}

fn render_range_with_edits(
    source: &str,
    range: std::ops::Range<usize>,
    edits: &[(std::ops::Range<usize>, String)],
) -> String {
    let mut result = String::new();
    let mut position = range.start;
    for (edit, text) in edits {
        if edit.start < range.start || edit.end > range.end {
            continue;
        }
        result.push_str(source.get(position..edit.start).unwrap());
        result.push_str(text);
        position = edit.end;
    }
    result.push_str(source.get(position..range.end).unwrap());
    result
}
