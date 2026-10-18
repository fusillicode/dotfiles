//! Ordering preferences across production, test-only, macro, and source-boundary scopes.

pub mod production {
    //! Item sections, visibility, impl adjacency, callers, and attached comments.

    extern crate core;

    fn unrelated() {}

    use std::fmt;
    pub use std::io;

    mod private_external;
    pub mod public_external;

    unsafe extern "C" {
        fn foreign_function();
    }

    global_asm!("");

    pub mod declarations {
        #[cfg(test)]
        mod tests;

        mod child;
    }

    pub mod nested {
        //! Each inline module and inherent impl has its own ordering.

        const PRIVATE: u8 = 0;
        pub(super) static RESTRICTED: u8 = 1;
        pub(crate) const CRATE: u8 = 2;
        pub const PUBLIC: u8 = 3;

        type Private = u8;
        pub(super) type Restricted = u8;
        pub(crate) type Crate = u8;
        pub type Public = u8;

        fn shared() {}

        fn private_caller() {
            shared();
        }

        pub(super) fn restricted() {}

        pub(crate) fn crate_visible() {}

        struct PrivateRecord;

        impl Display for Record {
            fn display(&self) -> u8 {
                self.value
            }
        }

        /// The record and both inherent impls must stay together.
        pub struct Record {
            value: u8,
        }

        pub fn first() {
            shared();
        }

        impl Record {
            fn normalize(&mut self) {
                self.value = 0;
            }

            pub(super) fn restricted(&self) -> u8 {
                self.value
            }

            pub(crate) fn crate_visible(&self) -> u8 {
                self.value
            }

            pub fn reset(&mut self) {
                self.normalize();
            }
        }

        pub fn second() {
            shared();
        }

        impl Record {
            const EMPTY: u8 = 0;

            fn validate(&self) -> bool {
                self.value == 0
            }

            pub fn is_valid(&self) -> bool {
                self.validate()
            }
        }

        pub trait Display {
            fn display(&self) -> u8;
        }

        mod private_inline {}

        pub mod public_inline {}

        #[cfg(test)]
        mod tests {
            fn sample() -> u8 {
                0
            }

            #[test]
            fn record_starts_empty() {
                let _ = sample();
            }
        }
    }

    type PrivateAlias = u8;

    const PRIVATE_LIMIT: u8 = 1;

    pub static PUBLIC_LIMIT: u8 = 2;

    pub type PublicAlias = u8;

    // Private helper documentation travels with its function.
    fn helper() -> u8 {
        1
    } // Keep this trailing comment with helper.

    /// Public entry documentation travels with its function.
    pub fn entry() -> u8 {
        helper()
    }

    pub enum Mode {
        Ready,
    }

    pub union Storage {
        byte: u8,
        word: u32,
    }

    #[test]
    fn production_test_stays_after_helpers() {
        let _ = helper();
    }

    #[cfg(test)]
    mod tests {
        fn sample() -> u8 {
            1
        }

        #[test]
        fn entry_returns_sample() {
            let _ = sample();
        }
    }
}

#[cfg(test)]
pub mod tests {
    //! Test-only scopes put tests before helpers, including inside nested modules and impls.

    fn sample() -> u8 {
        1
    }

    fn build() -> u8 {
        sample()
    }

    #[test]
    fn first_uses_builder() {
        let _ = build();
    }

    #[rstest::rstest]
    #[case(1)]
    fn parameterized_uses_sample(#[case] expected: u8) {
        let _ = (sample(), expected);
    }

    pub mod nested {
        fn helper() {}

        pub struct Fixture;

        impl Fixture {
            fn helper() {}

            #[test]
            fn example() {
                Self::helper();
            }
        }

        #[custom::integration_test]
        fn nested_test() {
            helper();
        }
    }

    #[cfg(test)]
    mod tests {
        fn helper() {}

        #[test]
        fn example() {
            helper();
        }
    }

    // Attribute arguments do not turn this helper into a test.
    #[tracing::instrument(name = "test")]
    fn traced_helper() {}

    pub mod recursive_tests {
        #[test]
        fn first() {
            second();
        }

        fn helper() {}

        #[test]
        fn second() {
            first();
        }
    }

    pub mod sections {
        fn helper() {
            example();
        }

        #[test]
        fn example() {
            helper();
        }

        fn unrelated() {}
    }
}

pub mod exceptions {
    //! Ordering exceptions preserve macro binding and independent safe inner fixes.

    pub mod imported_macros {
        fn private_before() {}

        pub fn public_before() {}

        #[macro_use]
        mod macros;

        fn private_after() {}

        pub fn public_after() {}
    }

    pub mod conditional_macro_imports {
        mod before;

        #[cfg_attr(feature = "first", macro_use)]
        mod first;

        pub mod between;

        #[macro_use]
        mod second;

        pub mod after;
    }

    pub mod local_references {
        fn direct() {}

        fn attribute() {}

        fn assertion() -> bool {
            true
        }

        fn nested_tokens() {}

        fn stringified() {}

        fn identifier() {}

        fn macro_name() {}

        fn text_only() {}

        fn qualified_only() {}

        fn nested_only() {}

        #[case(attribute())]
        fn caller() {
            self::direct();
            assert!(assertion());
            custom!(nested => [self::nested_tokens()]);
            stringify!(stringified());
            custom!(identifier);

            macro_name!();
            custom!("text_only()");
            custom!($qualified_only, foreign::qualified_only(), ::qualified_only(), self::nested::qualified_only());

            fn nested() {
                nested_only();
            }
        }
    }

    pub mod helper_chain {
        fn leaf() {}

        fn middle() {
            leaf();
        }

        fn unrelated() {}

        pub fn entry() {
            middle();
        }
    }

    pub mod recursive_block {
        fn odd(n: u32) -> bool {
            n != 0 && even(n - 1)
        }

        pub fn parity(n: u32) -> bool {
            even(n)
        }

        pub struct Data {
            pub value: u32,
        }

        impl Data {
            pub fn new(value: u32) -> Self {
                Self { value }
            }
        }

        fn even(n: u32) -> bool {
            n == 0 || odd(n - 1)
        }
    }

    pub mod macro_shadowing {
        pub mod child {
            macro_rules! m {
                () => {
                    pub const INNER: () = ();
                };
            }

            m!();
        }

        macro_rules! m {
            () => {
                pub const OUTER: () = ();
            };
        }

        pub mod sibling {
            m!();
        }
    }

    pub mod imported_then_local_macro {
        #[macro_use]
        mod macros;

        mod before {
            m!();
        }

        macro_rules! m {
            () => {};
        }

        mod after {
            m!();
        }
    }

    pub mod type_crosses_macro_source {
        struct Data;

        macro_rules! value {
            () => {
                7
            };
        }

        impl Data {
            fn value() -> u8 {
                value!()
            }
        }
    }

    pub mod type_crosses_opaque_source {
        struct Data;

        declare_macros!();

        impl Data {
            fn value() -> u8 {
                value!()
            }
        }
    }

    pub mod unused_macro_between_type_and_impl {
        struct Data;

        macro_rules! unused {
            () => {};
        }

        impl Data {}
    }

    pub mod conflicting_outer_scope {
        #[custom]
        struct Data;

        macro_rules! m {
            () => {};
        }

        impl Data {
            fn private() {
                m!();
            }

            pub fn public() {}
        }
    }

    pub mod production_sections {
        fn helper() {
            example();
        }

        #[test]
        fn example() {
            helper();
        }

        fn unrelated() {}
    }

    #[cfg(test)]
    pub mod verification {
        mod nested {
            struct Data;

            impl Data {
                pub fn helper() {}

                #[test]
                fn example() {}
            }

            fn helper() {}

            #[test]
            fn example() {}
        }
    }

    pub mod macro_sections {
        macro_rules! m {
            () => {
                pub const V: u8 = super::VALUE;
            };
        }

        pub mod child {
            m!();
        }

        const VALUE: u8 = 1;
    }
}

pub mod test_module_placement {
    #[cfg(test)]
    pub mod test {
        fn helper() {}

        #[test]
        fn example() {}
    }

    #[cfg(test)]
    mod checks {}

    fn production() {}

    const VALUE: u8 = 1;

    mod ordinary {}

    pub mod external_declarations {
        const VALUE: u8 = 1;

        #[cfg(test)]
        mod test;

        #[cfg(test)]
        mod tests;

        #[cfg(test)]
        mod checks;
    }

    pub mod macro_conflict {
        macro_rules! value {
            () => {
                1
            };
        }

        #[cfg(test)]
        mod test {
            fn helper() -> u8 {
                value!()
            }

            #[test]
            fn example() {
                assert_eq!(helper(), 1);
            }
        }

        macro_rules! value {
            () => {
                2
            };
        }
    }
}

pub mod inline_module_placement {
    //! Inline modules follow other declarations, alphabetically, with tests last regardless of visibility.

    pub mod tests {}

    pub mod zebra {}

    mod external;

    #[cfg(test)]
    mod checks {}

    // This comment and the nested module move together.
    mod alpha {
        mod tests {}

        pub mod zebra {}

        mod alpha {}

        fn entry() {}
    }

    fn entry() {}
}

pub mod alphabetical_macro_conflict {
    //! Preserve macro bindings when alphabetical module order would change them.

    #[macro_use]
    mod zebra {
        macro_rules! m {
            () => {};
        }
    }

    mod alpha {
        m!();
    }
}

pub mod boundaries {
    //! Preserve comments, Unicode, same-line boundaries, and conditional declarations.

    pub mod comments {
        //! Scope documentation stays in this module.

        // Blank-separated scope header stays in its original slot.

        // Private documentation stays with alpha.
        #[cfg(unix)]
        fn α() {} /* first */
        // private tail

        /* Public documentation */
        /// Public API documentation stays with beta.
        pub fn β() {}
    }

    pub mod same_line {
        fn α() {}
        pub fn β() {}
    }

    pub mod inner_docs {
        #![allow(dead_code)]
        /* Private note stays with its function. */
        fn private() {}

        pub fn public() {}
    }

    pub mod nested_comments {
        fn private() {} /* trailing
        //
        /* nested */
         */

        pub fn public() {}
    }

    pub mod conditional_types {
        #[cfg(feature = "first")]
        pub struct Data;

        #[cfg(not(feature = "first"))]
        pub struct Data;

        pub fn entry() -> Data {
            Data::new()
        }

        impl Data {
            pub fn new() -> Self {
                Self
            }
        }
    }

    pub mod repeated_impl_labels {
        pub struct Data;

        #[cfg(unix)]
        impl Data {
            fn private() {}

            pub fn public() {}
        }

        #[cfg(windows)]
        impl Data {
            fn private() {}

            pub fn public() {}
        }
    }
}
