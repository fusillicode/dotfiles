//! Source rewrites and conservative skips for unqualified-call.

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
