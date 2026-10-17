//! Source rewrites and conservative skips for aliased-import.

pub mod private_alias {
    use std::fmt::Debug as DPrivateAlias;
    fn run(_: &dyn DPrivateAlias) {}
}

pub mod module_alias {
    use std::fmt as format_module_alias;
    fn run() -> format_module_alias::Result {
        Ok(())
    }
}

pub mod attribute_macro_alias {
    use macros::my_attr as attr_attribute_macro_alias;
    #[attr_attribute_macro_alias]
    fn run() {}
}

pub mod grouped_self_alias {
    use std::fmt::{self as format_grouped_self_alias, Debug as DGroupedSelfAlias};
    fn run(_: &dyn DGroupedSelfAlias) -> format_grouped_self_alias::Result {
        Ok(())
    }
}

pub mod wildcard_alias {
    use std::fmt::Debug as _;
}

pub mod alias_clash {
    use std::fmt::Debug as DAliasClash;
    struct Debug;
    fn run(_: &dyn DAliasClash) {}
}

pub mod exported_alias {
    pub use std::fmt::Debug as DExportedAlias;
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

pub mod alias_in_child {
    use std::fmt::Debug as DAliasInChild;
    mod child {
        fn run(_: &dyn super::DAliasInChild) {}
    }
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

pub mod nested_aliases_claim_same_name {
    use root::{
        first::{Thing as ANestedAliasesClaimSameName},
        second::{Thing as BNestedAliasesClaimSameName},
    };
    fn run(_: ANestedAliasesClaimSameName, _: BNestedAliasesClaimSameName) {}
}
