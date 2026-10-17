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
