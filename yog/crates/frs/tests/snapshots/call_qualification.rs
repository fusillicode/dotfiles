//! Source rewrites and conservative skips for call-qualification.

pub mod same_module_calls {
    mod child;

    fn helper<T>() {}

    fn run() {
        self::helper::<u8>();
        crate::fixture::same_module_calls::helper::<u8>();
    }

    fn recursive() {
        crate::fixture::same_module_calls::recursive();
    }

    fn parameter_shadow(helper: fn()) {
        self::helper::<u8>();
        crate::fixture::same_module_calls::helper::<u8>();
    }

    fn local_shadow() {
        self::helper::<u8>();
        let helper = || ();
        self::helper::<u8>();
    }

    fn block_function_shadow() {
        self::helper::<u8>();
        fn helper() {}
    }

    fn block_import_shadow() {
        use other::helper;
        self::helper::<u8>();
    }

    fn constructor_shadow() {
        struct helper;
        self::helper::<u8>();
    }

    fn separate_namespaces() {
        struct helper { value: u8 }
        self::helper::<u8>();
    }

    fn unknown_bindings() {
        generate_bindings!();
        self::helper::<u8>();
    }

    fn preserved_comment() {
        self /* keep */ ::helper::<u8>();
    }

    fn different_module() {
        crate::elsewhere::helper();
    }

    mod nested {
        fn helper() {}

        fn run() {
            self::helper();
            crate::fixture::same_module_calls::nested::helper();
            super::helper::<u8>();
        }
    }
}

fn root_function() {}

pub mod attribute_function_import {
    use facade::drop;
    #[drop]
    fn run() {
        drop(1);
    }
}

pub mod attributed_import_in_another_namespace {
    #[cfg(all())]
    pub use other as twin;
    pub fn run() {
        m::twin();
    }

    pub mod m {
        pub fn twin() {}
    }

    pub mod other {}
}

pub mod block_const {
    use std::mem::drop as drop_block_const;
    fn run() {
        const drop_block_const: fn(u8) = |_| ();
        drop_block_const(1);
    }
}

pub mod block_glob {
    use std::env::current_dir as current_dir_block_glob;
    fn run() {
        use local::*;
        current_dir_block_glob(".");
    }
}

pub mod block_import {
    use std::env::current_dir as current_dir_block_import;
    fn run() {
        use std::fs::canonicalize as current_dir_block_import;
        current_dir_block_import(".");
    }
}

pub mod block_static {
    use std::mem::drop as drop_block_static;
    fn run() {
        static drop_block_static: fn(u8) = |_| ();
        drop_block_static(1);
    }
}

pub mod both_call_spellings {
    use std::fs;


    pub fn run() {
        let _ = fs::read_to_string("x");
        let _ = fs::read_to_string("x");
    }
}

pub mod commented_import {
    use std::mem::{drop /* keep this comment */};

    pub fn run() {
        drop(String::new());
    }
}

pub mod conditional_call_needing_an_import {
    #[cfg(any())]
    pub fn disabled() {
        let _ = std::fs::read_to_string("x");
    }
}

pub mod conditional_remaining_reference {
    use m::foo;
    pub fn run() {
        foo();
    }

    #[cfg(any())]
    pub fn disabled() {
        let _f = foo;
    }
    pub mod m {
        pub fn foo() {}
    }
}

pub mod conditional_statement_reference {
    use m::foo;
    pub fn run() {
        foo();
    }

    pub fn other() {
        #[cfg(any())]
        let _f = foo;
    }
    pub mod m {
        pub fn foo() {}
    }
}

pub mod conflicting_block_module {
    use std::mem::drop as discard;
    fn run() {
        use other::mem;
        discard(1);
    }
}

pub mod crate_import {
    use crate::parent;

    fn entry() {
        parent::run();
    }
}

pub mod crate_import_beside_local_module {
    use crate::helpers::call;
    fn run() {
        call();
    }
    mod helpers {}
}

pub mod crate_root_function {

    fn entry() {
        crate::root_function();
    }
}

pub mod external_crate_and_function_import {
    use std::mem;
    use std::mem::drop as std;

    pub fn run() {
        std(String::new());
        let _ = mem::size_of::<u8>();
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

pub mod external_references {
    use std::mem::drop as drop_external_references;
    use std::mem;
    mod child;
    fn run() {
        drop_external_references(1);
    }
}

pub mod function_name_binding {
    use std::mem;

    fn run() {
        mem::drop(1);
    }

    fn other(drop_function_name_binding: fn(u8)) {}
}

pub mod glob {
    use unknown::*;
    fn run() {
        helper();
    }
}

pub mod glob_could_capture_module {
    use std::fs::read_to_string;
    use unknown::*;
    fn run() {
        read_to_string("x");
    }
}

pub mod grouped_fn_import {
    use std::mem;

    fn run() {
        mem::drop(1);
        mem::size_of::<u8>();
        mem::align_of::<u8>();
    }
}

pub mod import_path_dependency {
    use common::module_idx;
    pub fn run() -> bool {
        let idx = module_idx(0);
        module_idx::has_external_modules(idx)
    }


    pub mod common {
        pub fn module_idx(_: usize) -> usize {
            0
        }
        pub mod module_idx {
            pub fn has_external_modules(_: usize) -> bool {
                false
            }
        }
    }
}

pub mod initializer_uses_outer_binding {
    use std::mem;

    fn run() {
        let discard = { mem::drop(1); |_: u8| () };
        discard(2);
    }
}

pub mod keep_other_fn_references {
    use std::mem;
    use std::mem::drop;
    fn run() {
        let f = drop::<u8>;
        mem::drop(1);
        f(2);
    }
}

pub mod known_block_module {

    fn run() {
        use std::mem;
        mem::drop(1);
    }
}

pub mod known_module_with_glob {
    use std::fs;

    use unknown::*;
    fn run() {
        fs::read_to_string("x");
    }
}

pub mod let_binding_order {
    use std::mem;

    fn run() {
        mem::drop(1);
        let discard = |_: u8| ();
        discard(2);
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

pub mod let_chain_else {
    use std::mem;

    fn run() {
        if let Some(discard) = None::<fn(u8)> && { discard(1); true } {
            discard(2);
        } else {
            mem::drop(3);
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

pub mod local_fn {
    fn run() {
        helper();
        self::helper();
    }
    fn helper() {}
}

pub mod local_let {
    use std::mem::drop as drop_local_let;
    fn run() {
        let drop_local_let = |_: u8| ();
        drop_local_let(1);
    }
}

pub mod macro_bindings {
    use std::mem::drop as drop_macro_bindings;
    fn run() {
        make_bindings!();
        drop_macro_bindings(1);
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

pub mod match_guard_binding {
    use std::mem;

    fn run() {
        match () {
            _ if let Some(discard) = None::<fn(u8)> => discard(1),
            _ => mem::drop(2),
        }
    }
}

pub mod module_import_replaces_function_binding {
    use std::fs;
    use std::mem;


    pub fn run() {
        mem::drop(String::new());
        let _ = fs::read_to_string("x");
    }
}

pub mod module_name_binding {
    use std::mem;

    fn run(mem: u8) {
        mem::drop(1);
    }
}

pub mod nested_attribute_function_reference {
    use facade::drop as drop_nested_attribute_function_reference;
    #[keep(nested(drop_nested_attribute_function_reference))]
    fn run() {
        drop_nested_attribute_function_reference(1);
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
        mem::drop(2);
    }
}

pub mod nested_item_function {
    use std::mem;

    fn run() {
        fn nested() {
            mem::drop(1);
        }
        let discard = |_: u8| ();
        discard(2);
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
        mem::drop(String::new());
    }
}

pub mod parameter {
    use std::mem::drop as drop_parameter;
    fn run(drop_parameter: fn(u8)) {
        drop_parameter(1);
    }
}

pub mod parent_function {

    fn entry() {
        super::root_function();
    }
}

pub mod parent_import_beside_local_module {
    use super::helpers::call;
    fn run() {
        call();
    }
    mod helpers {}
}

pub mod qualify_aliased_fn {
    use std::mem;

    fn run() {
        mem::drop(1);
    }
}

pub mod qualify_imported_fn {
    use std::fs;

    fn run() {
        fs::read_to_string("x");
    }
}

pub mod qualify_only_bare_calls {
    use std::mem;

    fn run() {
        mem::drop(1);
        mem::size_of::<u8>();
    }
}

pub mod remove_nested_group {
    use std::mem;
    use std::{

        fs::File,
    };
    fn run(_: File) {
        mem::drop(1);
        mem::size_of::<u8>();
    }
}

pub mod retain_grouped_type {
    use std::fs;
    use std::fs::{File};
    fn run(_: File) {
        fs::read_to_string("x");
    }
}

pub mod retain_nested_group_reference {
    use std::mem;
    use std::{
        mem::{
             size_of,

        },
        fs::File,
    };
    fn run(_: File) {
        let f = size_of::<u8>;
        mem::drop(1);
        mem::align_of::<u8>();
        let _ = f();
    }
}

pub mod same_spelling_in_type_namespace {
    use std::mem;

    fn run() {
        struct discard { value: u8 }
        let value: discard = discard { value: 1 };
        mem::drop(value);
    }
}

pub mod self_import {
    fn run() {
        helpers::call();
    }

    mod helpers {
        fn call() {}
    }
}

pub mod shared_function_and_module {
    use common::module_idx;
    pub fn run() -> bool {
        let idx = common::module_idx(0);
        module_idx::has_external_modules(idx)
    }


    pub mod common {
        pub fn module_idx(_: usize) -> usize {
            0
        }
        pub mod module_idx {
            pub fn has_external_modules(_: usize) -> bool {
                false
            }
        }
    }
}

mod shortened_calls {
    //! Source rewrites and conservative skips for call-qualification.

    pub mod associated {
        use std::path::PathBuf;
        fn run() {
            PathBuf::new();
        }
    }

    pub mod block_type {
        fn run() {
            std::fs::read_to_string("x");
            mod fs {}
        }
    }

    pub mod call_comment {
        fn run() {
            std /* explanation */ ::mem::drop(1);
        }
    }

    pub mod crate_call {
        use crate::parent::nested;
        fn run() {
            nested::run();
        }
    }

    pub mod existing_type_reference {
        fn run(value: &str) {
            let _ = str::len(value);
            std::str::from_utf8(b"x");
        }
    }

    pub mod explicit_parent_path {
        use super::parent::nested;
        fn run() {
            nested::run();
        }
    }

    pub mod generated_type_bindings {
        #[derive(MakeBindings)]
        struct Input;
        fn run() {
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

    pub mod macro_from_glob {
        use std::fs;
        use unknown::*;
        fn run() {
            std::println!("reading");
            std::fs::read_to_string("x");
        }
    }

    pub mod macro_qualifier_capture {
        fn run() {
            std::println!("reading");
            facade::std::call();
        }
    }

    pub mod name_clash {
        fn run() {
            std::fs::read_to_string("x");
        }
        mod fs {}
    }

    pub mod opaque_attribute_beside_known_module {
        use std::fs;
        #[make_bindings]
        fn other() {}
        fn run() {
            fs::read_to_string("x");
        }
    }

    pub mod opaque_macro {
        fn run() {
            make_bindings!();
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

    pub mod preserve_turbofish {
        use std::mem;
        fn run() {
            mem::size_of::<u32>();
        }
    }

    pub mod procedural_attribute {
        #[make_bindings]
        fn run() {
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
            fs::read_to_string("x");
        }
    }

    pub mod qualify_in_inline_scope {
        mod child {
            use std::fs;
            fn run() {
                fs::read_to_string("x");
            }
        }
    }

    pub mod self_call {
        use self::helpers::inner;
        fn run() {
            inner::call();
        }
        mod helpers {
            mod inner {
                fn call() {}
            }
        }
    }

    pub mod shadowed_source_prefix {
        fn run() {
            mod std {}
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

    pub mod short_module {
        fn run() {
            fs::read_to_string("x");
        }
    }

    pub mod shorten_beside_local_callable {
        use std::mem;
        use std::mem::drop;
        fn run(drop: fn(u8)) {
            drop(1);
            mem::size_of::<u8>();
        }
    }

    pub mod shorten_only_qualified_calls {
        use std::mem;

        fn run() {
            mem::drop(1);
            mem::size_of::<u8>();
        }
    }

    pub mod shorten_with_external_module {
        use std::mem::drop;
        mod child;
        fn run() {
            drop(1);
            std::mem::size_of::<u8>();
        }
    }

    pub mod standard_macros {
        use std::fs;
        fn run() {
            println!("reading");
            fs::read_to_string("x");
        }
    }

    pub mod type_namespace {
        fn run<fs>() {
            std::fs::read_to_string("x");
        }
    }

    pub mod unrelated_scopes {
        use std::fs;
        fn other() {
            make_bindings!();
            let fs = "local value";
        }
        fn run() {
            fs::read_to_string("x");
        }
    }

    pub mod value_namespace {
        use std::fs;
        fn fs() {}
        fn run(fs: u8) {
            fs::read_to_string("x");
        }
    }
}

pub mod standard_macros {
    use std::fs;

    fn run() {
        println!("reading");
        fs::read_to_string("x");
    }
}

pub mod type_declaration_and_function_import {
    use std::mem;
    pub fn run() {
        mem::drop(String::new());
    }


    pub mod discard {}
}

pub mod type_parameter {
    use std::mem::drop as discard;
    fn run<mem>() {
        discard(1);
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

pub mod unresolved_imports {
    use std::mem::drop as drop_unresolved_imports;
    use unknown::*;
    fn run() {
        drop_unresolved_imports(1);
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
