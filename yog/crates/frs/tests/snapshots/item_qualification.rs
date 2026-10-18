//! Source rewrites and conservative skips for item-qualification.

pub mod associated_constructor {
    use std::path::PathBuf;
    fn run() {
        let _ = PathBuf::new();
    }
}

pub mod avoid_duplicate_import {
    use std::path::PathBuf;
    fn run(_: PathBuf) -> PathBuf {
        PathBuf::new()
    }
}

pub mod conditional_import {
    #[cfg(unix)]
    use a::Thing;
    fn run(_: b::Other) {}
}

pub mod crate_type {
    struct Data;
    fn run(_: crate::Data) {}
}

pub mod function_call {
    use std::mem;
    fn run() {
        mem::drop(1);
    }
}

pub mod function_values {
    use std::path::PathBuf;
    fn run(text: &str) {
        let _ = text.trim_matches(char::is_whitespace);
        let _ = Some(text).map(str::trim);
        let _ = [1].into_iter().for_each(std::mem::drop);
        let _ = std::mem::size_of::<PathBuf>;
    }
}

pub mod generic_clash {
    fn run<PathBuf>(_: std::path::PathBuf) {}
}

pub mod import_constant {
    use std::u32::MAX;
    fn run() -> u32 {
        MAX
    }
}

pub mod import_enum_receiver {
    use std::sync::atomic::Ordering;
    fn run() {
        let _ = Ordering::Relaxed;
    }
}

pub mod import_type {
    use std::path::PathBuf;
    fn run(_: PathBuf) {}
}

pub mod macro_generated_import {
    m!();
    fn run(_: std::path::PathBuf) {}
}

pub mod macro_tokens {
    fn run() {
        m!(std::path::PathBuf);
    }
}

pub mod preserve_generics {
    use std::collections::HashMap;
    fn run() -> HashMap<String, u8> {
        HashMap::<String, u8>::new()
    }
}

pub mod protected_names {
    fn custom(
        _: external::Error,
        _: external::String,
        _: external::Result<()>,
        _: external::Option<()>,
        _: external::Vec<u8>,
        _: external::Box<u8>,
    ) -> String {
        String::new()
    }

    fn constructors() {
        let _ = external::String::new();
        let _ = external::Error::new();
    }

    fn traits(_: impl external::Clone + external::Future<Output = ()>) {}
}

pub mod qualified_block_glob {
    fn run(_: std::path::PathBuf) {
        use local::*;
    }
}

pub mod qualified_clash {
    struct PathBuf;
    fn run(_: std::path::PathBuf) {}
}

pub mod qualified_comment {
    fn run(_: std /* explanation */ ::path::PathBuf) {}
}

pub mod qualified_result_aliases {
    fn run() -> std::io::Result<std::fmt::Result> {
        Ok(Ok(()))
    }

    fn core_format() -> core::fmt::Result {
        Ok(())
    }

    fn custom(_: anyhow::Result<()>, _: rootcause::Result<()>) {}
}

pub mod same_module_const_clash {
    const VALUE: u8 = 1;
    fn run<const VALUE: u8>() -> u8 {
        crate::VALUE
    }
}

pub mod same_module_generic_clash {
    struct Data;
    fn run<Data>(_: crate::Data) {}
}

pub mod same_name_imports {
    use a::Thing;
    fn run(_: Thing, _: b::Thing) {}
}

pub mod self_generic_shadow {
    pub struct Data;

    pub fn run<Data>(_: self::Data, _: Data) {}
}

pub mod self_type {
    struct Data;
    fn run(_: Data) {}
}

pub mod standard_import_conflict {
    use std::time::Duration;
    fn run(_: external::Duration) -> Duration {
        Duration::ZERO
    }
}

pub mod standard_names {
    fn run(_: std::string::String, _: alloc::vec::Vec<u8>, _: core::option::Option<()>, _: std::io::Error) {}
}

pub mod standard_result {
    fn run(_: std::result::Result<(), ()>) {}
}

pub mod unqualified_enum {
    enum Kind {
        Value,
    }
    fn run() -> Kind {
        Kind::Value
    }
}

pub mod source_visible_macros {
    fn run(_: std::path::PathBuf) {
        arbitrary_macro!();
    }

    #[custom_attribute]
    fn attributed(_: std::time::Instant) {}
}

pub mod conditional_name_collision {
    #[cfg(unix)]
    use other::PathBuf;

    fn run(_: std::path::PathBuf, _: std::time::Instant) {}
}

pub mod conditional_source_binding {
    #[cfg(unix)]
    use std as platform;

    fn run(_: platform::path::PathBuf) {}
}

pub mod conditional_wildcard {
    #[cfg(unix)]
    use other::*;

    fn run(_: std::path::PathBuf) {}
}

pub mod primitive_associated_constants {
    fn limits() {
        let _ = i8::MAX;
        let _ = i16::MAX;
        let _ = i32::MAX;
        let _ = i64::MAX;
        let _ = i128::MAX;
        let _ = isize::MAX;
        let _ = u8::MAX;
        let _ = u16::MAX;
        let _ = u32::MAX;
        let _ = u64::MAX;
        let _ = u128::MAX;
        let _ = usize::MAX;
        let _ = f16::INFINITY;
        let _ = f32::INFINITY;
        let _ = f64::INFINITY;
        let _ = f128::INFINITY;
        let _ = char::MAX;
        let _ = std::primitive::i64::MAX;
        let _ = core::primitive::f64::INFINITY;
    }

    fn convert(value: usize) -> i64 {
        i64::try_from(value).unwrap_or(i64::MAX)
    }
}
