//! Source rewrites and conservative skips for import-aliasing.

pub mod alias_clash {
    use std::fmt::Debug as DAliasClash;
    struct Debug;
    fn run(_: &dyn DAliasClash) {}
}

pub mod alias_in_child {
    use std::fmt::Debug as DAliasInChild;
    mod child {
        fn run(_: &dyn super::DAliasInChild) {}
    }
}

pub mod alias_in_macro {
    use std::fmt::Debug as DAliasInMacro;
    m!(DAliasInMacro);
}

pub mod alias_in_nested_macro {
    use std::fmt::Debug as DAliasInNestedMacro;
    m!({ [(DAliasInNestedMacro)] });
}

pub mod alias_shadow {
    use std::fmt::Debug as DAliasShadow;
    fn run<DAliasShadow>(_: DAliasShadow) {}
}

pub mod alias_to_external_child {
    use std::fmt::Debug as DAliasToExternalChild;
    mod child;
}

pub mod alias_to_nested_external_child {
    use std::fmt::Debug as DAliasToNestedExternalChild;
    mod child {
        mod leaf;
    }
}

pub mod attribute_macro_alias {
    use macros::my_attr;
    #[my_attr]
    fn run() {}
}

pub mod exported_alias {
    pub use std::fmt::Debug as DExportedAlias;
}

pub mod grouped_self_alias {
    use std::fmt::{self, Debug};
    fn run(_: &dyn Debug) -> fmt::Result {
        Ok(())
    }
}

pub mod module_alias {
    use std::fmt;
    fn run() -> fmt::Result {
        Ok(())
    }
}

pub mod nested_aliases_claim_same_name {
    use root::{
        first::{Thing},
        second::{Thing as BNestedAliasesClaimSameName},
    };
    fn run(_: Thing, _: BNestedAliasesClaimSameName) {}
}

pub mod private_alias {
    use std::fmt::Debug;
    fn run(_: &dyn Debug) {}
}

pub mod wildcard_alias {
    use std::fmt::Debug as _;
}
