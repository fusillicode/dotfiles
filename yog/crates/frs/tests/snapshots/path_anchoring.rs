//! Source rewrites and conservative skips for path-anchoring.

pub mod multiple_parent_levels {
    mod a {
        mod b {
            use {super::super::Thing, super::Other};
        }
    }
}

pub mod nested_relative_groups {
    mod a {
        mod b {
            use {
                super::Thing,
                {super::super::Other, super::Third},
            };
        }
    }
}

pub mod relative_import_and_type {
    mod a {
        mod b {
            use super::Thing;
            fn run(_: super::Thing) {
                super::run();
            }
        }
    }
}

pub mod test_glob {
    #[cfg(test)]
    mod tests {
        use super::*;
        mod nested {
            use super::*;
        }
    }
}

pub mod unresolved_parent {
    use super::Thing;
}

pub mod self_paths {
    use self::inner::Thing;

    mod inner {
        pub struct Thing;
    }

    pub(super) fn run(_: self::inner::Thing) {}

    struct Container;
    impl Container {
        fn unchanged(&self) -> &Self {
            self
        }
    }
}

pub mod local_module {
    fn run() {
        mod local {
            use super::Thing;
        }
    }
}

pub mod visibility_context {
    mod parent {
        pub(in super::super::visibility_context) mod child {}
    }
}

pub mod nested_type_arguments {
    mod inner {
        fn run(_: self::Container<super::Thing>) {}
    }
}

pub mod preserve_comments {
    mod inner {
        use super /* keep */ ::super::Thing;
    }
}

use super::OutsideCrate;
