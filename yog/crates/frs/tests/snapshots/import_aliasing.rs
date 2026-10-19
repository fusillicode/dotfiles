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

pub mod source_visible_macros {
    #[allow(unused_imports)]
    use std::path::PathBuf as FilePath;

    fn run(_: FilePath) {
        arbitrary_macro!();
    }
}

pub mod conditional_explicit_imports {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf as ConditionalBuffer;

    fn run(_: ConditionalBuffer) {}
}

pub mod conditional_alias {
    #[cfg(unix)]
    use std::path::PathBuf as ConditionalAliasBuffer;

    fn run(_: ConditionalAliasBuffer) {}
}

pub mod conditional_name_collision {
    #[cfg(unix)]
    use other::PathBuf;
    use std::path::PathBuf as CollisionBuffer;

    fn run(_: CollisionBuffer) {}
}

pub mod alias_in_quoted_attribute {
    use crate::serde_utils as utils;

    #[derive(Deserialize)]
    struct CommandArgs {
        #[serde(deserialize_with = "utils::empty_string_is_none")]
        args: Option<String>,
    }
}

pub mod alias_in_raw_attribute {
    use crate::serde_utils as raw_utils;

    #[custom(nested(path = r#"raw_utils::deserialize"#))]
    struct CommandArgs;
}

pub mod alias_in_escaped_attribute {
    use crate::serde_utils as escaped_utils;

    #[custom(path = "\u{65}scaped_utils::deserialize")]
    struct CommandArgs;
}

pub mod alias_in_name_value_attribute {
    use crate::serde_utils as attribute_utils;

    #[custom = "attribute_utils::deserialize"]
    struct CommandArgs;
}

pub mod alias_in_quoted_macro {
    use crate::serde_utils as macro_utils;

    custom!({ [(r#"macro_utils::deserialize"#)] });
}

pub mod alias_in_non_code_attribute_text {
    use crate::serde_utils as text_utils;

    #[custom(description = "use text_utils::deserialize({")]
    struct CommandArgs;
}

pub mod unrelated_quoted_attribute {
    use std::path::PathBuf as AttributePath;

    #[custom(path = "other::deserialize")]
    fn run(_: AttributePath) {}
}

pub mod alias_in_ordinary_string {
    use std::path::PathBuf as OrdinaryPath;

    fn run(_: OrdinaryPath) -> &'static str {
        "OrdinaryPath is plain text here"
    }
}
