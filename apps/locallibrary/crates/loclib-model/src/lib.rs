#![doc = include_str!("../README.md")]

pub mod analysis;
pub mod harmony;
pub mod model;
pub mod paths;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

pub use model::{Library, Playlist, Track, TrackColor};

/// A struct whose JSON is an array of its fields in order (`["Contents/a.mp3", "unreadable"]`),
/// the shape the result had as a tuple, while its schema (and the TypeScript types made from it)
/// names each element (`[path: string, error: string]`). Doc comments on the fields become the
/// elements' descriptions. The schema is there with the `schemars` feature.
///
/// ```
/// loclib_model::labelled_tuple! {
///     /// A tempo range: `[low, high]` in BPM.
///     pub struct Range {
///         pub low: f64,
///         pub high: f64,
///     }
/// }
///
/// let json = serde_json::to_string(&Range { low: 120.0, high: 128.0 }).unwrap();
/// assert_eq!(json, "[120.0,128.0]");
/// ```
#[macro_export]
macro_rules! labelled_tuple {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[doc = $doc:literal])*
                $fvis:vis $field:ident: $ty:ty
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[doc = $doc])*
                $fvis $field: $ty,
            )+
        }

        impl $crate::__private::serde::Serialize for $name {
            fn serialize<S: $crate::__private::serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                $crate::__private::serde::Serialize::serialize(&($(&self.$field,)+), serializer)
            }
        }

        impl<'de> $crate::__private::serde::Deserialize<'de> for $name {
            fn deserialize<D: $crate::__private::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> std::result::Result<Self, D::Error> {
                let ($($field,)+) =
                    <($($ty,)+) as $crate::__private::serde::Deserialize>::deserialize(deserializer)?;
                Ok($name { $($field),+ })
            }
        }

        $crate::__labelled_tuple_schema! { $name { $($(#[doc = $doc])* $field: $ty),+ } }
    };
}

/// [`labelled_tuple!`]'s schema, chosen by this crate's `schemars` feature (a `cfg` in the macro
/// itself would be the calling crate's).
#[cfg(feature = "schemars")]
#[doc(hidden)]
#[macro_export]
macro_rules! __labelled_tuple_schema {
    ($name:ident { $($(#[doc = $doc:literal])* $field:ident: $ty:ty),+ }) => {
        impl $crate::__private::schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                concat!(module_path!(), "::", stringify!($name)).into()
            }

            fn json_schema(
                generator: &mut $crate::__private::schemars::SchemaGenerator,
            ) -> $crate::__private::schemars::Schema {
                let items: Vec<$crate::__private::schemars::Schema> = vec![$({
                    let mut item = generator.subschema_for::<$ty>();
                    item.insert("title".into(), stringify!($field).into());
                    let doc: &[&str] = &[$($doc),*];
                    let doc = doc.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
                    if !doc.is_empty() {
                        item.insert("description".into(), doc.into());
                    }
                    item
                }),+];
                let n = items.len();
                $crate::__private::schemars::json_schema!({
                    "type": "array",
                    "prefixItems": items,
                    "minItems": n,
                    "maxItems": n,
                })
            }
        }
    };
}

#[cfg(not(feature = "schemars"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __labelled_tuple_schema {
    ($($tokens:tt)*) => {};
}

/// What [`labelled_tuple!`] expands to refers to, so the crates using it needn't depend on these.
#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "schemars")]
    pub use schemars;
    pub use serde;
}
