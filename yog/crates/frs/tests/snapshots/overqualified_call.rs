//! Source rewrites and conservative skips for overqualified-call.

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
