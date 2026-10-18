//! Source rewrites and conservative skips for call-qualification.

pub mod qualify_imported_fn {
    use std::fs::read_to_string;
    fn run() {
        read_to_string("x");
    }
}

pub mod keep_other_fn_references {
    use std::mem::drop as drop_keep_other_fn_references;
    fn run() {
        let f = drop_keep_other_fn_references::<u8>;
        drop_keep_other_fn_references(1);
        f(2);
    }
}

pub mod grouped_fn_import {
    use std::mem::{size_of as size_of_grouped_fn_import, drop, align_of};
    fn run() {
        drop(1);
        size_of_grouped_fn_import::<u8>();
        align_of::<u8>();
    }
}

pub mod retain_grouped_type {
    use std::fs::{File, read_to_string as read_to_string_retain_grouped_type};
    fn run(_: File) {
        read_to_string_retain_grouped_type("x");
    }
}

pub mod qualify_aliased_fn {
    use std::mem::drop as discard_qualify_aliased_fn;
    fn run() {
        discard_qualify_aliased_fn(1);
    }
}

pub mod remove_nested_group {
    use std::{
        mem::{drop as drop_remove_nested_group, size_of as size_of_remove_nested_group},
        fs::File,
    };
    fn run(_: File) {
        drop_remove_nested_group(1);
        size_of_remove_nested_group::<u8>();
    }
}

pub mod retain_nested_group_reference {
    use std::{
        mem::{
            drop as drop_retain_nested_group_reference, size_of as size_of_retain_nested_group_reference,
            align_of as align_of_retain_nested_group_reference,
        },
        fs::File,
    };
    fn run(_: File) {
        let f = size_of_retain_nested_group_reference::<u8>;
        drop_retain_nested_group_reference(1);
        align_of_retain_nested_group_reference::<u8>();
        let _ = f();
    }
}

pub mod qualify_only_bare_calls {
    use std::mem::drop as drop_qualify_only_bare_calls;
    fn run() {
        drop_qualify_only_bare_calls(1);
        std::mem::size_of::<u8>();
    }
}

pub mod self_import {
    mod helpers {
        fn call() {}
    }
    use self::helpers::call;
    fn run() {
        call();
    }
}

pub mod attribute_function_import {
    use facade::drop as drop_attribute_function_import;
    #[drop_attribute_function_import]
    fn run() {
        drop_attribute_function_import(1);
    }
}

pub mod nested_attribute_function_reference {
    use facade::drop as drop_nested_attribute_function_reference;
    #[keep(nested(drop_nested_attribute_function_reference))]
    fn run() {
        drop_nested_attribute_function_reference(1);
    }
}

pub mod local_fn {
    fn helper() {}
    fn run() {
        helper();
        self::helper();
    }
}

pub mod parameter {
    use std::mem::drop as drop_parameter;
    fn run(drop_parameter: fn(u8)) {
        drop_parameter(1);
    }
}

pub mod local_let {
    use std::mem::drop as drop_local_let;
    fn run() {
        let drop_local_let = |_: u8| ();
        drop_local_let(1);
    }
}

pub mod let_chain {
    use std::mem::drop as drop_let_chain;
    fn run() {
        if true && let Some(drop_let_chain) = Some(|_: u8| ()) {
            drop_let_chain(1);
        }
    }
}

pub mod local_block_fn {
    use std::mem::drop as drop_local_block_fn;
    fn run() {
        drop_local_block_fn(1);
        fn drop_local_block_fn(_: u8) {}
    }
}

pub mod glob {
    use unknown::*;
    fn run() {
        helper();
    }
}

pub mod crate_import_beside_local_module {
    use crate::helpers::call as call_crate_import_beside_local_module;
    mod helpers {}
    fn run() {
        call_crate_import_beside_local_module();
    }
}

pub mod parent_import_beside_local_module {
    use super::helpers::call as call_parent_import_beside_local_module;
    mod helpers {}
    fn run() {
        call_parent_import_beside_local_module();
    }
}

pub mod block_import {
    use std::env::current_dir as current_dir_block_import;
    fn run() {
        use std::fs::canonicalize as current_dir_block_import;
        current_dir_block_import(".");
    }
}

pub mod block_glob {
    use std::env::current_dir as current_dir_block_glob;
    fn run() {
        use local::*;
        current_dir_block_glob(".");
    }
}

pub mod block_const {
    use std::mem::drop as drop_block_const;
    fn run() {
        const drop_block_const: fn(u8) = |_| ();
        drop_block_const(1);
    }
}

pub mod block_static {
    use std::mem::drop as drop_block_static;
    fn run() {
        static drop_block_static: fn(u8) = |_| ();
        drop_block_static(1);
    }
}

pub mod external_references {
    use std::mem::drop as drop_external_references;
    use std::mem;
    mod child;
    fn run() {
        drop_external_references(1);
    }
}

pub mod crate_import {
    use crate::parent::run;
    fn entry() {
        run();
    }
}

pub mod macro_bindings {
    use std::mem::drop as drop_macro_bindings;
    fn run() {
        make_bindings!();
        drop_macro_bindings(1);
    }
}

pub mod unresolved_imports {
    use std::mem::drop as drop_unresolved_imports;
    use unknown::*;
    fn run() {
        drop_unresolved_imports(1);
    }
}

pub mod module_name_binding {
    use std::mem::drop as drop_module_name_binding;
    fn run(mem: u8) {
        drop_module_name_binding(1);
    }
}

pub mod function_name_binding {
    use std::mem::drop as drop_function_name_binding;
    fn run() {
        drop_function_name_binding(1);
    }

    fn other(drop_function_name_binding: fn(u8)) {}
}

pub mod standard_macros {
    use std::fs::read_to_string;
    fn run() {
        println!("reading");
        read_to_string("x");
    }
}

pub mod unrelated_macro {
    use std::fs;
    use std::fs::read_to_string;
    fn other() {
        make_bindings!();
    }
    fn run() {
        read_to_string("x");
    }
}

pub mod known_module_with_glob {
    use std::fs;
    use std::fs::read_to_string;
    use unknown::*;
    fn run() {
        read_to_string("x");
    }
}

pub mod glob_could_capture_module {
    use std::fs::read_to_string;
    use unknown::*;
    fn run() {
        read_to_string("x");
    }
}

pub mod let_binding_order {
    use std::mem::drop as discard;
    fn run() {
        discard(1);
        let discard = |_: u8| ();
        discard(2);
    }
}

pub mod nested_block {
    use std::mem;
    use std::mem::drop as discard;
    fn run() {
        {
            mod mem {}
            discard(1);
        }
        discard(2);
    }
}

pub mod let_chain_else {
    use std::mem::drop as discard;
    fn run() {
        if let Some(discard) = None::<fn(u8)> && { discard(1); true } {
            discard(2);
        } else {
            discard(3);
        }
    }
}

pub mod initializer_uses_outer_binding {
    use std::mem::drop as discard;
    fn run() {
        let discard = { discard(1); |_: u8| () };
        discard(2);
    }
}

pub mod nested_item_function {
    use std::mem::drop as discard;
    fn run() {
        fn nested() {
            discard(1);
        }
        let discard = |_: u8| ();
        discard(2);
    }
}

pub mod external_crate_qualifier {
    use rootcause::report;
    mod child;
    fn run() {
        report("failure");
    }
}

pub mod external_import_insertion {
    use std::mem::drop as discard;
    mod child;
    fn run() {
        discard(1);
    }
}

pub mod type_parameter {
    use std::mem::drop as discard;
    fn run<mem>() {
        discard(1);
    }
}

pub mod known_block_module {
    use std::mem::drop as discard;
    fn run() {
        use std::mem;
        discard(1);
    }
}

pub mod conflicting_block_module {
    use std::mem::drop as discard;
    fn run() {
        use other::mem;
        discard(1);
    }
}

pub mod opaque_attribute_reference {
    use std::mem;
    use std::mem::drop as discard;
    #[make_reference]
    fn other() {}
    fn run() {
        discard(1);
    }
}

pub mod match_guard_binding {
    use std::mem::drop as discard;
    fn run() {
        match () {
            _ if let Some(discard) = None::<fn(u8)> => discard(1),
            _ => discard(2),
        }
    }
}

pub mod crate_root_function {
    use crate::root_function as run;
    fn entry() {
        run();
    }
}

pub mod parent_function {
    use super::root_function as run;
    fn entry() {
        run();
    }
}

fn root_function() {}

pub mod same_spelling_in_type_namespace {
    use std::mem::drop as discard;
    fn run() {
        struct discard { value: u8 }
        let value: discard = discard { value: 1 };
        discard(value);
    }
}

mod shortened_calls {
    //! Source rewrites and conservative skips for call-qualification.

    pub mod preserve_turbofish {
        fn run() {
            std::mem::size_of::<u32>();
        }
    }

    pub mod shorten_only_qualified_calls {
        use std::mem::drop;
        fn run() {
            drop(1);
            std::mem::size_of::<u8>();
        }
    }

    pub mod shorten_with_external_module {
        mod child;
        use std::mem::drop;
        fn run() {
            drop(1);
            std::mem::size_of::<u8>();
        }
    }

    pub mod shorten_beside_local_callable {
        use std::mem::drop;
        fn run(drop: fn(u8)) {
            drop(1);
            std::mem::size_of::<u8>();
        }
    }

    pub mod qualify_in_inline_scope {
        mod child {
            fn run() {
                std::fs::read_to_string("x");
            }
        }
    }

    pub mod self_call {
        mod helpers {
            mod inner {
                fn call() {}
            }
        }
        fn run() {
            self::helpers::inner::call();
        }
    }

    pub mod short_module {
        fn run() {
            fs::read_to_string("x");
        }
    }

    pub mod associated {
        fn run() {
            std::path::PathBuf::new();
        }
    }

    pub mod name_clash {
        mod fs {}
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod call_comment {
        fn run() {
            std /* explanation */ ::mem::drop(1);
        }
    }

    pub mod crate_call {
        fn run() {
            crate::parent::nested::run();
        }
    }

    pub mod unrelated_scopes {
        use std::fs;
        fn other() {
            make_bindings!();
            let fs = "local value";
        }
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod standard_macros {
        fn run() {
            println!("reading");
            std::fs::read_to_string("x");
        }
    }

    pub mod opaque_macro {
        fn run() {
            make_bindings!();
            std::fs::read_to_string("x");
        }
    }

    pub mod shadowed_standard_macro {
        macro_rules! println {
            () => { mod fs {} };
        }
        fn run() {
            println!();
            std::fs::read_to_string("x");
        }
    }

    pub mod qualified_standard_macro {
        use std::fs;
        macro_rules! println {
            () => { mod fs {} };
        }
        fn run() {
            std::println!("reading");
            std::fs::read_to_string("x");
        }
    }

    pub mod value_namespace {
        fn fs() {}
        fn run(fs: u8) {
            std::fs::read_to_string("x");
        }
    }

    pub mod type_namespace {
        fn run<fs>() {
            std::fs::read_to_string("x");
        }
    }

    pub mod impl_type_parameter {
        struct Reader;
        impl<fs> Reader {
            fn run() {
                std::fs::read_to_string("x");
            }
        }
    }

    pub mod block_type {
        fn run() {
            std::fs::read_to_string("x");
            mod fs {}
        }
    }

    pub mod existing_type_reference {
        fn run(value: &str) {
            let _ = str::len(value);
            std::str::from_utf8(b"x");
        }
    }

    pub mod explicit_parent_path {
        fn run() {
            super::parent::nested::run();
        }
    }

    pub mod shadowed_source_prefix {
        fn run() {
            mod std {}
            std::fs::read_to_string("x");
        }
    }

    pub mod procedural_attribute {
        #[make_bindings]
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod generated_type_bindings {
        #[derive(MakeBindings)]
        struct Input;
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod opaque_attribute_beside_known_module {
        use std::fs;
        #[make_bindings]
        fn other() {}
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod macro_from_glob {
        use std::fs;
        use unknown::*;
        fn run() {
            std::println!("reading");
            std::fs::read_to_string("x");
        }
    }

    pub mod opaque_macro_may_observe_new_import {
        fn other() {
            let _ = generated_expression!();
        }
        fn run() {
            std::fs::read_to_string("x");
        }
    }

    pub mod inherited_macro {
        macro_rules! println {
            () => { mod fs {} };
        }
        mod child {
            fn run() {
                println!();
                std::fs::read_to_string("x");
            }
        }
    }

    pub mod macro_qualifier_capture {
        fn run() {
            std::println!("reading");
            facade::std::call();
        }
    }
}

pub mod shared_function_and_module {
    pub mod common {
        pub mod module_idx {
            pub fn has_external_modules(_: usize) -> bool {
                false
            }
        }
        pub fn module_idx(_: usize) -> usize {
            0
        }
    }
    use common::module_idx;
    use common::module_idx::has_external_modules;

    pub fn run() -> bool {
        let idx = module_idx(0);
        has_external_modules(idx)
    }
}

pub mod both_call_spellings {
    use std::fs::read_to_string;

    pub fn run() {
        let _ = read_to_string("x");
        let _ = std::fs::read_to_string("x");
    }
}

pub mod opaque_expansion {
    use std::mem;
    use std::mem::drop as discard;

    macro_rules! hidden {
        () => {{ 1usize }};
    }

    pub fn other() -> usize {
        hidden!()
    }
    pub fn run() {
        let _ = mem::size_of::<u8>();
        discard(String::new());
    }
}

pub mod opaque_expansion_with_remaining_use {
    use std::mem;
    use std::mem::drop as discard;

    macro_rules! hidden {
        () => {{ 1usize }};
    }

    pub fn other() -> usize {
        hidden!()
    }
    pub fn pointer() -> fn(String) {
        discard
    }
    pub fn run() {
        let _ = mem::size_of::<u8>();
        discard(String::new());
    }
}

pub mod type_declaration_and_function_import {
    pub mod discard {}
    use std::mem::drop as discard;

    pub fn run() {
        discard(String::new());
    }
}

pub mod value_declaration_and_module_import {
    use std::fs;

    pub fn fs() {}
    pub fn run() {
        fs();
        let _ = fs::read_to_string("x");
    }
}

pub mod commented_import {
    use std::mem::{drop /* keep this comment */};

    pub fn run() {
        drop(String::new());
    }
}

pub mod attributed_import_in_another_namespace {
    pub mod m {
        pub fn twin() {}
    }
    pub mod other {}
    use m::twin;
    #[cfg(all())]
    pub use other as twin;

    pub fn run() {
        twin();
    }
}

pub mod import_path_dependency {
    pub mod common {
        pub mod module_idx {
            pub fn has_external_modules(_: usize) -> bool {
                false
            }
        }
        pub fn module_idx(_: usize) -> usize {
            0
        }
    }
    use common::module_idx;
    use module_idx::has_external_modules;

    pub fn run() -> bool {
        let idx = module_idx(0);
        has_external_modules(idx)
    }
}

pub mod external_crate_and_function_import {
    use std::mem::drop as std;

    pub fn run() {
        std(String::new());
        let _ = std::mem::size_of::<u8>();
    }
}

pub mod module_import_replaces_function_binding {
    use std::mem::drop as fs;

    pub fn run() {
        fs(String::new());
        let _ = std::fs::read_to_string("x");
    }
}

pub mod conditional_remaining_reference {
    pub mod m {
        pub fn foo() {}
    }
    use m::foo;

    pub fn run() {
        foo();
    }
    #[cfg(any())]
    pub fn disabled() {
        let _f = foo;
    }
}

pub mod conditional_call_needing_an_import {
    #[cfg(any())]
    pub fn disabled() {
        let _ = std::fs::read_to_string("x");
    }
}

pub mod conditional_statement_reference {
    pub mod m {
        pub fn foo() {}
    }
    use m::foo;

    pub fn run() {
        foo();
    }
    pub fn other() {
        #[cfg(any())]
        let _f = foo;
    }
}

pub mod macro_shadowed_remaining_reference {
    use std::mem;
    use std::mem::drop as discard;

    macro_rules! local_discard {
        () => {
            fn discard(_: String) {}
        };
    }

    pub fn pointer() -> fn(String) {
        local_discard!();
        discard
    }
    pub fn run() {
        let _ = mem::size_of::<u8>();
        discard(String::new());
    }
}
