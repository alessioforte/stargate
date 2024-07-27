#[macro_export]
macro_rules! model {
    (
        $(#[$attr: meta])*
        $vis: vis struct $name: ident {
            $(
                $(#[$field_attr: meta])*
                $field_vis: vis $field: ident: $type: ty
            ),* $(,)?
        }
        $(,{
            $(
                $(#[$add_field_attr: meta])*
                $add_field_vis: vis $add_field: ident: $add_type: ty
            ),* $(,)?
        })?
    ) => {
        $(#[$attr])*
        $vis struct $name {
            #[serde(deserialize_with = "thing_to_string")]
            pub id: String,
            $(
                $(#[$field_attr])*
                $field_vis $field: $type,
            )*

            $($(
                $(#[$add_field_attr])*
                $add_field_vis $add_field: $add_type,
            )*)?
        }

        $(#[$attr])*
        $vis struct Payload {
            $(
                $(#[$field_attr])*
                $field_vis $field: $type,
            )*
        }
    };
}
