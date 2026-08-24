/// Declare an enum of unit variants together with its `ALL` array.
///
/// The variant list in the declaration is the only list: `ALL` is generated
/// from it, so a new variant joins every `ALL`-driven iteration for free and
/// cannot be silently omitted the way a hand maintained parallel array can.
/// That matters most for `#[non_exhaustive]` enums, whose variants downstream
/// crates cannot enumerate at all.
///
/// Attributes pass through unchanged, on both the enum and its variants, so
/// derives, `#[non_exhaustive]`, serde attributes, and doc comments are written
/// exactly as they would be on a plain declaration.
///
/// ```
/// lilo_common::define_unit_enum! {
///     /// Why a request was refused.
///     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
///     pub enum Refusal {
///         Unauthenticated,
///         Forbidden,
///     }
/// }
///
/// assert_eq!(Refusal::ALL, [Refusal::Unauthenticated, Refusal::Forbidden]);
/// ```
#[macro_export]
macro_rules! define_unit_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: [Self; [$(stringify!($variant)),+].len()] =
                [$(Self::$variant),+];
        }
    };
}
