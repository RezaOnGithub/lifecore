//! The derives of typed decoding: `FromBody` for structs with named fields,
//! `FromPayload` for enums, and `Declare` for a struct that lists a type's
//! canonical fields. SPEC §6.5 owns what they generate and their
//! `#[lifecore(…)]` attributes. `lifecore::decode` re-exports them beside the
//! traits of the same names, so depend on `lifecore`, not on this crate.
//!
//! `Option<T>` is recognized by its last path segment, so a type alias of it
//! is not; such a field then fails to compile, since `Option<T>` is no
//! payload type. `FromBody` accepts `Declare`'s type attributes, so one
//! struct may derive both.
//!
//! Generated code names only absolute paths: `::core`, `::std`, and public
//! items of `lifecore` through `::lifecore`, or through the path of the
//! `crate` attribute. Field types keep their own spans, so a field whose type
//! cannot be decoded is reported at that type.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{quote, quote_spanned};
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprArray, Fields, FieldsNamed, Ident, Lit, LitStr, Path,
    Type, ext::IdentExt, parse_macro_input, spanned::Spanned,
};

#[proc_macro_derive(FromBody, attributes(lifecore))]
pub fn derive_from_body(input: TokenStream) -> TokenStream {
    expand(input, from_body)
}

#[proc_macro_derive(FromPayload, attributes(lifecore))]
pub fn derive_from_payload(input: TokenStream) -> TokenStream {
    expand(input, from_payload)
}

#[proc_macro_derive(Declare, attributes(lifecore))]
pub fn derive_declare(input: TokenStream) -> TokenStream {
    expand(input, declare)
}

fn expand(input: TokenStream, derive: fn(&DeriveInput) -> syn::Result<Tokens>) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn from_body(input: &DeriveInput) -> syn::Result<Tokens> {
    let container = Container::parse(&input.attrs)?;
    let fields = named_fields(input, "FromBody", "derive `FromPayload` for an enum")?;
    let krate = &container.krate;
    let name = &input.ident;
    let decls = field_decls(krate, &fields);
    let reads = fields.iter().map(|field| field.read(krate, None));
    Ok(quote! {
        impl #krate::decode::FromBody for #name {
            fn fields() -> ::std::collections::BTreeMap<
                ::std::string::String,
                #krate::typing::FieldDecl,
            > {
                #decls
            }
            fn from_body(
                body: &#krate::Body,
            ) -> ::core::result::Result<Self, #krate::decode::DecodeError> {
                ::core::result::Result::Ok(Self { #(#reads,)* })
            }
        }
        impl #krate::decode::FromPayload for #name {
            fn shape() -> #krate::typing::ValueType {
                #krate::typing::ValueType::Record {
                    fields: <Self as #krate::decode::FromBody>::fields(),
                }
            }
            fn from_payload(
                value: &#krate::Payload,
            ) -> ::core::result::Result<Self, #krate::decode::DecodeError> {
                <Self as #krate::decode::FromBody>::from_body(#krate::decode::record(value)?)
            }
        }
    })
}

fn from_payload(input: &DeriveInput) -> syn::Result<Tokens> {
    let container = Container::parse(&input.attrs)?;
    container.only_crate("FromPayload")?;
    no_generics(input, "FromPayload")?;
    let data = match &input.data {
    Data::Enum(data) => data,
    Data::Struct(_) => {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "derive `FromBody` for a struct; it decodes the struct from a record as well",
        ));
    }
    Data::Union(_) => {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[derive(FromPayload)]` does not support unions",
        ));
    }
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[derive(FromPayload)]` needs at least one variant: an empty enum or variant has no value",
        ));
    }
    let mut variants = vec![];
    let mut names = Names::default();
    for variant in &data.variants {
        let rename = VariantAttrs::parse(&variant.attrs)?.rename;
        let key = match rename {
        Some(rename) => {
            if rename.value().is_empty() {
                return Err(syn::Error::new_spanned(
                    rename,
                    "a member or tag name cannot be empty",
                ));
            }
            rename.value()
        }
        None => snake_case(&variant.ident.unraw().to_string()),
        };
        names.insert(&key, variant.ident.span(), "member or tag name")?;
        let fields = match &variant.fields {
        Fields::Unit => None,
        Fields::Named(named) => Some(field_infos(named)?),
        Fields::Unnamed(unnamed) => {
            return Err(syn::Error::new_spanned(
                unnamed,
                "a variant's fields need names, which its tag's body is keyed by",
            ));
        }
        };
        variants.push((variant.ident.clone(), key, fields));
    }
    let krate = &container.krate;
    let name = &input.ident;
    let unit_only = variants.iter().all(|(_, _, fields)| fields.is_none());
    let (shape, decode) = if unit_only {
        let members = variants.iter().map(|(_, key, _)| key);
        let arms = variants.iter().map(|(ident, key, _)| {
            quote! { #key => ::core::result::Result::Ok(Self::#ident), }
        });
        (
            quote! {
                #krate::typing::ValueType::Enum {
                    members: ::std::collections::BTreeSet::from([
                        #(::std::string::String::from(#members),)*
                    ]),
                }
            },
            quote! {
                match #krate::decode::text(value)? {
                    #(#arms)*
                    other => ::core::result::Result::Err(#krate::decode::DecodeError::new(
                        #krate::decode::DecodeProblem::NotAMember {
                            value: ::std::string::String::from(other),
                        },
                    )),
                }
            },
        )
    } else {
        let tags = variants.iter().map(|(_, key, fields)| {
            let decls = field_decls(krate, fields.as_deref().unwrap_or_default());
            quote! { (::std::string::String::from(#key), #decls), }
        });
        let arms = variants.iter().map(|(ident, key, fields)| {
            let reads = fields
                .iter()
                .flatten()
                .map(|field| field.read(krate, Some(key)));
            quote! { #key => ::core::result::Result::Ok(Self::#ident { #(#reads,)* }), }
        });
        (
            quote! {
                #krate::typing::ValueType::Variant {
                    tags: ::std::collections::BTreeMap::from([#(#tags)*]),
                }
            },
            quote! {
                let (tag, body) = #krate::decode::variant(value)?;
                match tag {
                    #(#arms)*
                    other => ::core::result::Result::Err(#krate::decode::DecodeError::new(
                        #krate::decode::DecodeProblem::UnknownTag {
                            tag: ::std::string::String::from(other),
                        },
                    )),
                }
            },
        )
    };
    Ok(quote! {
        impl #krate::decode::FromPayload for #name {
            fn shape() -> #krate::typing::ValueType {
                #shape
            }
            fn from_payload(
                value: &#krate::Payload,
            ) -> ::core::result::Result<Self, #krate::decode::DecodeError> {
                #decode
            }
        }
    })
}

fn declare(input: &DeriveInput) -> syn::Result<Tokens> {
    let container = Container::parse(&input.attrs)?;
    let fields = named_fields(
        input,
        "Declare",
        "a declaration is a struct that lists every canonical field",
    )?;
    let Some(ty) = &container.ty else {
        return Err(syn::Error::new(
            Span::call_site(),
            "`#[derive(Declare)]` needs the declared type's name: `#[lifecore(type = \"…\")]`",
        ));
    };
    let krate = &container.krate;
    let name = &input.ident;
    let decls = field_decls(krate, &fields);
    let set = |names: &[String]| {
        quote! {
            ::std::collections::BTreeSet::from([#(::std::string::String::from(#names),)*])
        }
    };
    let outgoing = set(&container.outgoing);
    let incoming = set(&container.incoming);
    let successors = set(&container.successors);
    Ok(quote! {
        impl #krate::decode::Declare for #name {
            fn declaration() -> #krate::typing::Type {
                #krate::typing::Type {
                    name: ::std::string::String::from(#ty),
                    fields: #decls,
                    outgoing: #outgoing,
                    incoming: #incoming,
                    successors: #successors,
                }
            }
        }
    })
}

fn no_generics(input: &DeriveInput, derive: &str) -> syn::Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            format!("`#[derive({derive})]` does not support generic types"),
        ))
    }
}

fn named_fields(input: &DeriveInput, derive: &str, for_enum: &str) -> syn::Result<Vec<Field>> {
    no_generics(input, derive)?;
    match &input.data {
    Data::Struct(data) => match &data.fields {
    Fields::Named(named) => field_infos(named),
    _ => Err(syn::Error::new_spanned(
        &input.ident,
        format!("`#[derive({derive})]` needs a struct with named fields, which a body is keyed by"),
    )),
    },
    Data::Enum(_) => Err(syn::Error::new_spanned(
        &input.ident,
        format!("`#[derive({derive})]` is for structs; {for_enum}"),
    )),
    Data::Union(_) => Err(syn::Error::new_spanned(
        &input.ident,
        format!("`#[derive({derive})]` does not support unions"),
    )),
    }
}

#[derive(Clone, Copy)]
enum Status {
    Mandatory,
    Optional,
    Encouraged,
    Deprecated,
}

struct Field {
    ident: Ident,
    key: String,
    ty: Type,
    // The `T` of an `Option<T>` field.
    option: Option<Type>,
    status: Status,
}

impl Field {
    // `ident: <read>`, with errors under the tag when inside a variant.
    fn read(&self, krate: &Path, tag: Option<&String>) -> Tokens {
        let Self { ident, key, ty, .. } = self;
        let read = match &self.option {
        Some(inner) => quote_spanned! {ty.span()=>
            #krate::decode::optional_field::<#inner>(body, #key)
        },
        None => quote_spanned! {ty.span()=> #krate::decode::field::<#ty>(body, #key) },
        };
        match tag {
        None => quote! { #ident: #read? },
        Some(tag) => quote! {
            #ident: #read.map_err(|error| {
                error.under(#krate::typing::PathPart::Tag {
                    tag: ::std::string::String::from(#tag),
                })
            })?
        },
        }
    }
}

fn field_infos(named: &FieldsNamed) -> syn::Result<Vec<Field>> {
    let mut names = Names::default();
    let mut fields = vec![];
    for field in &named.named {
        let ident = field.ident.clone().expect("named field");
        let attrs = FieldAttrs::parse(&field.attrs)?;
        let key = attrs
            .rename
            .as_ref()
            .map_or_else(|| ident.unraw().to_string(), LitStr::value);
        names.insert(&key, ident.span(), "field name")?;
        let option = option_inner(&field.ty).cloned();
        let status = match (&option, attrs.presence) {
        (Some(_), None) => Status::Optional,
        (Some(_), Some((status, _))) => status,
        (None, None) => Status::Mandatory,
        (None, Some((_, word))) => {
            return Err(syn::Error::new_spanned(
                word,
                "a presence attribute applies only to an `Option<T>` field; a field read as `T` is mandatory",
            ));
        }
        };
        fields.push(Field {
            ident,
            key,
            ty: field.ty.clone(),
            option,
            status,
        });
    }
    Ok(fields)
}

// The field declarations of a body, as a `BTreeMap` expression.
fn field_decls(krate: &Path, fields: &[Field]) -> Tokens {
    let entries = fields.iter().map(|field| {
        let key = &field.key;
        let read = field.option.as_ref().unwrap_or(&field.ty);
        let shape = quote_spanned! {read.span()=> <#read as #krate::decode::FromPayload>::shape() };
        let status = match field.status {
        Status::Mandatory => quote!(Mandatory),
        Status::Optional => quote!(Optional),
        Status::Encouraged => quote!(Encouraged),
        Status::Deprecated => quote!(Deprecated),
        };
        quote! {
            (
                ::std::string::String::from(#key),
                #krate::typing::FieldDecl {
                    status: #krate::typing::FieldStatus::#status,
                    shape: #shape,
                },
            ),
        }
    });
    quote! { ::std::collections::BTreeMap::from([#(#entries)*]) }
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() {
        return None;
    }
    let last = path.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.iter().collect::<Vec<_>>().as_slice() {
    [syn::GenericArgument::Type(inner)] => Some(inner),
    _ => None,
    }
}

// As serde's `snake_case`: an underscore before every uppercase letter but
// the first, and everything lowercase.
fn snake_case(name: &str) -> String {
    let mut snake = String::new();
    for (i, c) in name.char_indices() {
        if i > 0 && c.is_uppercase() {
            snake.push('_');
        }
        snake.extend(c.to_lowercase());
    }
    snake
}

#[derive(Default)]
struct Names(Vec<String>);

impl Names {
    fn insert(&mut self, name: &str, span: Span, what: &str) -> syn::Result<()> {
        if self.0.iter().any(|seen| seen == name) {
            return Err(syn::Error::new(span, format!("duplicate {what} {name:?}")));
        }
        self.0.push(name.into());
        Ok(())
    }
}

struct Container {
    krate: Path,
    ty: Option<LitStr>,
    incoming: Vec<String>,
    outgoing: Vec<String>,
    successors: Vec<String>,
    // The first `Declare` key given, for derives that take none.
    declare_key: Option<Ident>,
}

impl Container {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut krate = None;
        let mut ty = None;
        let mut incoming = None;
        let mut outgoing = None;
        let mut successors = None;
        let mut declare_key = None;
        for attr in lifecore_attrs(attrs) {
            attr.parse_nested_meta(|meta| {
                let key = meta.path.get_ident().map(Ident::unraw);
                let Some(key) = key else {
                    return Err(meta.error("expected an attribute name"));
                };
                let duplicate = || meta.error(format!("duplicate attribute `{key}`"));
                match key.to_string().as_str() {
                "crate" => {
                    if krate.is_some() {
                        return Err(duplicate());
                    }
                    krate = Some(meta.value()?.parse::<LitStr>()?.parse::<Path>()?);
                }
                "type" => {
                    if ty.is_some() {
                        return Err(duplicate());
                    }
                    let name = meta.value()?.parse::<LitStr>()?;
                    if name.value() == "Type" {
                        return Err(syn::Error::new_spanned(
                            name,
                            "the name \"Type\" is reserved for the built-in metatype",
                        ));
                    }
                    ty = Some(name);
                    declare_key.get_or_insert(key);
                }
                word @ ("incoming" | "outgoing" | "successors") => {
                    let slot = match word {
                    "incoming" => &mut incoming,
                    "outgoing" => &mut outgoing,
                    _ => &mut successors,
                    };
                    if slot.is_some() {
                        return Err(duplicate());
                    }
                    *slot = Some(string_list(&meta.value()?.parse::<ExprArray>()?)?);
                    declare_key.get_or_insert(key);
                }
                _ => {
                    return Err(meta.error(
                        "unknown type attribute; expected `crate`, `type`, `incoming`, `outgoing`, or `successors`",
                    ));
                }
                }
                Ok(())
            })?;
        }
        Ok(Self {
            krate: krate.unwrap_or_else(|| syn::parse_quote!(::lifecore)),
            ty,
            incoming: incoming.unwrap_or_default(),
            outgoing: outgoing.unwrap_or_default(),
            successors: successors.unwrap_or_default(),
            declare_key,
        })
    }
    fn only_crate(&self, derive: &str) -> syn::Result<()> {
        match &self.declare_key {
        None => Ok(()),
        Some(key) => Err(syn::Error::new_spanned(
            key,
            format!(
                "`{key}` belongs to `#[derive(Declare)]` on a struct, not to `#[derive({derive})]`"
            ),
        )),
        }
    }
}

// Distinct string literals, for a permission set.
fn string_list(array: &ExprArray) -> syn::Result<Vec<String>> {
    let mut names = Names::default();
    let mut values = vec![];
    for element in &array.elems {
        let Expr::Lit(syn::ExprLit {
            lit: Lit::Str(name),
            ..
        }) = element
        else {
            return Err(syn::Error::new_spanned(
                element,
                "expected a string literal",
            ));
        };
        names.insert(&name.value(), name.span(), "label")?;
        values.push(name.value());
    }
    Ok(values)
}

struct FieldAttrs {
    rename: Option<LitStr>,
    // The policy, with the word that named it for errors.
    presence: Option<(Status, Ident)>,
}

impl FieldAttrs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut rename = None;
        let mut presence: Option<(Status, Ident)> = None;
        for attr in lifecore_attrs(attrs) {
            attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident().cloned() else {
                    return Err(meta.error("expected an attribute name"));
                };
                match key.to_string().as_str() {
                "rename" => {
                    if rename.is_some() {
                        return Err(meta.error("duplicate attribute `rename`"));
                    }
                    rename = Some(meta.value()?.parse::<LitStr>()?);
                }
                word @ ("encouraged" | "deprecated") => {
                    if let Some((_, earlier)) = &presence {
                        return Err(meta.error(format!(
                            "`{word}` conflicts with `{earlier}`: a field has one presence policy"
                        )));
                    }
                    let status = if word == "encouraged" {
                        Status::Encouraged
                    } else {
                        Status::Deprecated
                    };
                    presence = Some((status, key));
                }
                _ => {
                    return Err(meta.error(
                        "unknown field attribute; expected `rename`, `encouraged`, or `deprecated`",
                    ));
                }
                }
                Ok(())
            })?;
        }
        Ok(Self { rename, presence })
    }
}

struct VariantAttrs {
    rename: Option<LitStr>,
}

impl VariantAttrs {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut rename = None;
        for attr in lifecore_attrs(attrs) {
            attr.parse_nested_meta(|meta| {
                if !meta.path.is_ident("rename") {
                    return Err(meta.error("unknown variant attribute; expected `rename`"));
                }
                if rename.is_some() {
                    return Err(meta.error("duplicate attribute `rename`"));
                }
                rename = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            })?;
        }
        Ok(Self { rename })
    }
}

fn lifecore_attrs(attrs: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attrs.iter().filter(|attr| attr.path().is_ident("lifecore"))
}

#[cfg(test)]
mod test;
