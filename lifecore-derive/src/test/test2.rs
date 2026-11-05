// Misuse is a compile error with a message that says what to write instead.
// The derives' successful expansions are tested by what they generate, in
// `lifecore::decode`.

use crate::{declare, from_body, from_payload};
use proc_macro2::TokenStream;
use syn::{DeriveInput, parse_quote};

fn error(derive: fn(&DeriveInput) -> syn::Result<TokenStream>, input: DeriveInput) -> String {
    derive(&input)
        .expect_err("misuse is a compile error")
        .to_string()
}

#[test]
fn each_derive_names_the_one_that_fits() {
    assert_eq!(
        error(
            from_payload,
            parse_quote! { struct Stance { decision: Decision } }
        ),
        "derive `FromBody` for a struct; it decodes the struct from a record as well"
    );
    assert_eq!(
        error(from_body, parse_quote! { enum Per { Day } }),
        "`#[derive(FromBody)]` is for structs; derive `FromPayload` for an enum"
    );
    assert_eq!(
        error(declare, parse_quote! { enum Per { Day } }),
        "`#[derive(Declare)]` is for structs; a declaration is a struct that lists every canonical field"
    );
    assert_eq!(
        error(from_body, parse_quote! { struct Pair(String, i64); }),
        "`#[derive(FromBody)]` needs a struct with named fields, which a body is keyed by"
    );
    assert_eq!(
        error(from_body, parse_quote! { struct Wrapper<T> { value: T } }),
        "`#[derive(FromBody)]` does not support generic types"
    );
    assert_eq!(
        error(from_payload, parse_quote! { union Bits { a: u8 } }),
        "`#[derive(FromPayload)]` does not support unions"
    );
}

#[test]
fn enums_need_named_distinct_variants() {
    assert_eq!(
        error(from_payload, parse_quote! { enum Never {} }),
        "`#[derive(FromPayload)]` needs at least one variant: an empty enum or variant has no value"
    );
    assert_eq!(
        error(
            from_payload,
            parse_quote! { enum Decision { Committed(i64), Resumed {} } }
        ),
        "a variant's fields need names, which its tag's body is keyed by"
    );
    assert_eq!(
        error(
            from_payload,
            parse_quote! { enum Per { Day, #[lifecore(rename = "day")] Daily } }
        ),
        "duplicate member or tag name \"day\""
    );
    assert_eq!(
        error(
            from_payload,
            parse_quote! { enum Per { #[lifecore(rename = "")] Day } }
        ),
        "a member or tag name cannot be empty"
    );
    assert_eq!(
        error(
            from_payload,
            parse_quote! { enum Per { #[lifecore(skip)] Day } }
        ),
        "unknown variant attribute; expected `rename`"
    );
}

#[test]
fn field_attributes_are_checked() {
    assert_eq!(
        error(
            from_body,
            parse_quote! { struct Habit { #[lifecore(encouraged)] quota: i64 } }
        ),
        "a presence attribute applies only to an `Option<T>` field; a field read as `T` is mandatory"
    );
    assert_eq!(
        error(
            declare,
            parse_quote! {
                #[lifecore(type = "Habit")]
                struct Habit { #[lifecore(encouraged, deprecated)] note: Option<String> }
            }
        ),
        "`deprecated` conflicts with `encouraged`: a field has one presence policy"
    );
    assert_eq!(
        error(
            from_body,
            parse_quote! { struct Habit { quota: i64, #[lifecore(rename = "quota")] count: i64 } }
        ),
        "duplicate field name \"quota\""
    );
    assert_eq!(
        error(
            from_body,
            parse_quote! { struct Habit { #[lifecore(default)] quota: i64 } }
        ),
        "unknown field attribute; expected `rename`, `encouraged`, or `deprecated`"
    );
    assert_eq!(
        error(
            from_body,
            parse_quote! { struct Habit { #[lifecore(rename = "")] quota: i64 } }
        ),
        "a field name cannot be empty"
    );
}

#[test]
fn type_attributes_are_checked() {
    assert_eq!(
        error(declare, parse_quote! { struct Habit { quota: i64 } }),
        "`#[derive(Declare)]` needs the declared type's name: `#[lifecore(type = \"…\")]`"
    );
    assert_eq!(
        error(
            declare,
            parse_quote! { #[lifecore(type = "Type")] struct Meta {} }
        ),
        "the name \"Type\" is reserved for the built-in metatype"
    );
    assert_eq!(
        error(
            declare,
            parse_quote! { #[lifecore(type = "Stance", outgoing = ["About", "About"])] struct S {} }
        ),
        "duplicate label \"About\""
    );
    assert_eq!(
        error(
            declare,
            parse_quote! { #[lifecore(type = "Stance", outgoing = [About])] struct S {} }
        ),
        "expected a string literal"
    );
    assert_eq!(
        error(
            declare,
            parse_quote! { #[lifecore(type = "Stance", type = "Other")] struct S {} }
        ),
        "duplicate attribute `type`"
    );
    assert_eq!(
        error(
            from_payload,
            parse_quote! { #[lifecore(type = "Per")] enum Per { Day } }
        ),
        "`type` belongs to `#[derive(Declare)]` on a struct, not to `#[derive(FromPayload)]`"
    );
    assert_eq!(
        error(
            declare,
            parse_quote! { #[lifecore(name = "Stance")] struct S {} }
        ),
        "unknown type attribute; expected `crate`, `type`, `incoming`, `outgoing`, or `successors`"
    ); // An enum takes no `Declare` keys, so the message does not offer them.
    assert_eq!(
        error(
            from_payload,
            parse_quote! { #[lifecore(rename_all = "snake_case")] enum Per { Day } }
        ),
        "unknown type attribute; expected `crate`"
    );
    assert_eq!(
        error(
            from_body,
            parse_quote! { #[lifecore(crate = "not a path")] struct S {} }
        ),
        "`crate` takes the path of the `lifecore` crate, such as \"::my_lifecore\""
    );
}

#[test]
fn a_renamed_crate_is_named_in_the_expansion() {
    let input: DeriveInput = parse_quote! {
        #[lifecore(crate = "core_db")]
        enum Per { Day }
    };
    let expansion = from_payload(&input).unwrap().to_string();
    assert!(expansion.contains("impl core_db :: decode :: FromPayload for Per"));
    assert!(!expansion.contains("lifecore"));
}
