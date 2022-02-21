#![feature(once_cell)]

use proc_macro::TokenStream;
use std::collections::HashMap;
use std::lazy::SyncOnceCell;
use quote::{quote, TokenStreamExt};
use syn::{Field, Fields, FieldsNamed, FnArg, GenericArgument, Ident, ItemFn, ItemStruct, LitInt, parse_macro_input, PathArguments, ReturnType, Signature, Token, Type};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

static VERSIONS: SyncOnceCell<Vec<(String, u32)>> = SyncOnceCell::new();
fn get_versions() -> &'static Vec<(String, u32)> {
    VERSIONS.get_or_init(|| {
        let mut versions = Vec::new();
        let mut seen_header = false;
        for mut line in include_str!("../../res/versions.csv").lines() {
            if let Some(index) = line.find('#') {
                line = &line[..index];
            }
            line = line.trim();
            if line.is_empty() {
                continue;
            }
            if !seen_header {
                seen_header = true;
                continue;
            }

            let parts: Vec<_> = line.split(",").collect();
            versions.push((parts[0].to_string(), parts[1].parse::<u32>().unwrap()));
        }
        versions.sort_by_key(|(_, id)| !*id);
        versions
    })
}
static VERSIONS_BY_NAME: SyncOnceCell<HashMap<String, usize>> = SyncOnceCell::new();
fn get_versions_by_name() -> &'static HashMap<String, usize> {
    VERSIONS_BY_NAME.get_or_init(|| {
        let mut versions = HashMap::new();
        for (i, (name, _)) in get_versions().iter().enumerate() {
            versions.insert(name.clone(), i);
        }
        versions
    })
}

struct Variants {
    main: ItemStruct,
    variants: Vec<Variant>,
}

impl Parse for Variants {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let main: ItemStruct = input.parse()?;
        let mut variants: Vec<Variant> = Vec::new();
        while !input.is_empty() {
            variants.push(input.parse()?);
        }
        syn::Result::Ok(Variants { main, variants })
    }
}

struct Variant {
    up: ItemFn,
    down: ItemFn,
    version: Punctuated<LitInt, Token![,]>,
    _arrow_token: Token![=>],
    fields: FieldsNamed,
    _semi_token: Option<Token![;]>,
}

impl Parse for Variant {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let first_fn: ItemFn = input.parse()?;
        let first_name = first_fn.sig.ident.to_string();
        if first_name != "up" && first_name != "down" {
            return syn::Result::Err(syn::Error::new(
                first_fn.sig.ident.span(),
                "expected `up` or `down`",
            ));
        }
        let second_fn: ItemFn = input.parse()?;
        let second_name = second_fn.sig.ident.to_string();
        if second_name != "up" && second_name != "down" {
            return syn::Result::Err(syn::Error::new(
                second_fn.sig.ident.span(),
                "expected `up` or `down`",
            ));
        }
        if first_name == second_name {
            return syn::Result::Err(syn::Error::new(
                second_fn.sig.ident.span(),
                format!("encountered `{}` twice", second_name)
            ));
        }

        let (up_fn, down_fn) = if first_name == "up" {
            (first_fn, second_fn)
        } else {
            (second_fn, first_fn)
        };

        check_fn_sig(&up_fn.sig, "Self::UpInput", "Self::UpResult")?;
        check_fn_sig(&down_fn.sig, "Self::DownInput", "Self::DownResult")?;

        let version: Punctuated<LitInt, Token![,]> = Punctuated::parse_separated_nonempty(input)?;
        let arrow_token: Token![=>] = input.parse()?;
        let fields: FieldsNamed = input.parse()?;
        let semi_token: Option<Token![;]> = input.parse()?;

        syn::Result::Ok(Variant {
            up: up_fn,
            down: down_fn,
            version,
            _arrow_token: arrow_token,
            fields,
            _semi_token: semi_token,
        })
    }
}

fn check_fn_sig(sig: &Signature, expected_input: &str, expected_output: &str) -> syn::Result<()> {
    if sig.inputs.len() != 1 {
        return syn::Result::Err(syn::Error::new(
            sig.inputs.span(),
            format!("expected one input of type `{}`", expected_input),
        ));
    }
    match &sig.inputs[0] {
        FnArg::Typed(arg) => {
            let ty = &arg.ty;
            if quote!(#ty).to_string().replace(' ', "") != expected_input {
                return syn::Result::Err(syn::Error::new(
                    ty.span(),
                    format!("expected one input of type `{}`", expected_input),
                ));
            }
        }
        FnArg::Receiver(receiver) => {
            return syn::Result::Err(syn::Error::new(
                receiver.span(),
                format!("expected one input of type `{}`", expected_input),
            ));
        },
    }

    match &sig.output {
        ReturnType::Default => {
            return syn::Result::Err(syn::Error::new(
                sig.span(),
                format!("expected output of type `{}`", expected_output),
            ));
        }
        ReturnType::Type(_, ty) => {
            if quote!(#ty).to_string().replace(' ', "") != expected_output {
                return syn::Result::Err(syn::Error::new(
                    ty.span(),
                    format!("expected output of type `{}`", expected_output),
                ));
            }
        }
    }

    syn::Result::Ok(())
}

fn change_type<F>(ty: &mut Type, transformer: F) -> syn::Result<()>
where
    F: FnOnce(&str) -> String,
{
    if let Type::Path(path) = ty {
        if let Some(segment) = path.path.segments.last_mut() {
            if segment.ident == "Vec"
                || segment.ident == "HashMap"
                || segment.ident == "BTreeMap"
                || segment.ident == "AHashMap"
                || segment.ident == "FastDashMap"
            {
                if let PathArguments::AngleBracketed(args) = &mut segment.arguments {
                    if let Some(GenericArgument::Type(ty)) = args.args.last_mut() {
                        change_type(ty, transformer)?;
                    }
                }
            } else {
                segment.ident = syn::Ident::new(&transformer(&segment.ident.to_string()), segment.ident.span());
            }
        }
    } else {
        return syn::Result::Err(syn::Error::new(
            ty.span(),
            "expected a path",
        ));
    }

    syn::Result::Ok(())
}

#[proc_macro]
pub fn variants(items: TokenStream) -> TokenStream {
    let variants: Variants = parse_macro_input!(items);
    let mut main = variants.main;
    let main_ident = main.ident.clone();
    let mut variants = variants.variants;
    let fields = match &mut main.fields {
        Fields::Named(fields) => fields,
        _ => return syn::Error::new(
            main.span(),
            "expected named fields",
        ).to_compile_error().into(),
    };
    let tokens = quote!(
        _extra: std::collections::BTreeMap<String, nbt::Value>
    ).into();
    fields.named.push(parse_macro_input!(tokens with Field::parse_named));

    let mut variants_by_version: HashMap<_, &Variant> = HashMap::new();
    for variant in &mut variants {
        let tokens = quote!(
            #[serde(flatten)]
            _extra: std::collections::BTreeMap<String, nbt::Value>
        ).into();
        variant.fields.named.push(parse_macro_input!(tokens with Field::parse_named));
        let version = &variant.version;
        let version = quote!(#version).to_string().replace(' ', "").replace(',', ".");
        let version_id = match get_versions_by_name().get(&version) {
            Some(id) => *id,
            None => {
                return syn::Error::new(
                    version.span(),
                    format!("unknown version `{}`", version),
                ).to_compile_error().into();
            }
        };
        if variants_by_version.insert(version_id, variant).is_some() {
            return syn::Error::new(
                version.span(),
                format!("encountered version `{}` twice", version),
            ).to_compile_error().into();
        }
    }

    let mut current_variant = None;
    let mut all_versions = Vec::new();
    for index in 0..get_versions().len() {
        if let Some(variant) = variants_by_version.get(&index) {
            current_variant = Some(*variant);
        }
        all_versions.push(current_variant);
    }

    let mut output = quote!(
        #main
    );

    let mut up_struct = main.clone();
    for (index, variant) in all_versions.iter().enumerate() {
        let current_name = Ident::new(&format!("Variant_{}_{}", main_ident.to_string(), get_versions()[index].0.replace('.', "_")), main_ident.span());
        let mut the_struct = main.clone();
        the_struct.ident = current_name.clone();
        if let Some(variant) = variant {
            the_struct.fields = Fields::Named(variant.fields.clone());
        }
        if let Fields::Named(fields) = &mut the_struct.fields {
            for field in &mut fields.named {
                let has_variants = field.attrs.iter().any(|attr| attr.path.is_ident("variants"));
                if has_variants {
                    if let Err(e) = change_type(&mut field.ty, |s| format!("Variant_{}_{}", s, get_versions()[index].0.replace('.', "_"))) {
                        return e.to_compile_error().into();
                    }
                }
            }
        }
        output.append_all(quote!(
            #[derive(serde::Deserialize, serde::Serialize)]
            #[allow(non_camel_case_types)]
            #the_struct
        ));

        let custom_convert = if index == 0 {
            variant.is_some()
        } else {
            match (variant, all_versions[index - 1]) {
                (Some(variant), Some(prev_variant)) => {
                    let cur_ver = &variant.version;
                    let prev_ver = &prev_variant.version;
                    quote!(#cur_ver).to_string() != quote!(#prev_ver).to_string()
                },
                (None, None) => false,
                _ => true,
            }
        };

        let up_name = if index == 0 {
            main_ident.clone()
        } else {
            Ident::new(&format!("Variant_{}_{}", main_ident.to_string(), get_versions()[index - 1].0.replace('.', "_")), main_ident.span())
        };

        if custom_convert {
            let (up_fn, down_fn) = (&variant.unwrap().up, &variant.unwrap().down);
            output.append_all(quote!(
                impl crate::convert::Up for #up_name {
                    type UpInput = #current_name;
                    type UpResult = crate::convert::Result<Self>;
                    #up_fn
                }
                impl crate::convert::Down for #current_name {
                    type DownInput = #up_name;
                    type DownResult = crate::convert::Result<Self>;
                    #down_fn
                }
                impl std::convert::TryFrom<#current_name> for #up_name {
                    type Error = crate::convert::Error;
                    fn try_from(other: #current_name) -> crate::convert::Result<Self> {
                        <Self as crate::convert::Up>::up(other)
                    }
                }
                impl std::convert::TryFrom<#up_name> for #current_name {
                    type Error = crate::convert::Error;
                    fn try_from(other: #up_name) -> crate::convert::Result<Self> {
                        <Self as crate::convert::Down>::down(other)
                    }
                }
            ));
        } else {
            let (field_idents, (field_types, up_field_types)): (Vec<Ident>, (Vec<Type>, Vec<Type>)) = match (&the_struct.fields, &up_struct.fields) {
                (Fields::Named(fields), Fields::Named(up_fields)) =>
                    fields.named.iter().zip(&up_fields.named).map(|(field, up_field)|
                        (
                            field.ident.as_ref().unwrap().clone(),
                            (field.ty.clone(), up_field.ty.clone())
                        )
                    ).unzip(),
                _ => unreachable!(),
            };
            output.append_all(quote!(
                impl std::convert::TryFrom<#current_name> for #up_name {
                    type Error = crate::convert::Error;
                    fn try_from(other: #current_name) -> crate::convert::Result<Self> {
                        Ok(Self {
                            #(#field_idents: std::convert::TryFrom::<#field_types>::try_from(other.#field_idents).map_err(|e| std::convert::Into::<crate::convert::Error>::into(e))?,)*
                        })
                    }
                }
                impl std::convert::TryFrom<#up_name> for #current_name {
                    type Error = crate::convert::Error;
                    fn try_from(other: #up_name) -> crate::convert::Result<Self> {
                        Ok(Self {
                            #(#field_idents: std::convert::TryFrom::<#up_field_types>::try_from(other.#field_idents).map_err(|e| std::convert::Into::<crate::convert::Error>::into(e))?,)*
                        })
                    }
                }
            ));
        }

        up_struct = the_struct;
    }

    let mut deserialize_expr = quote!(
        return Err(serde::de::Error::custom("version is older than supported"));
    );
    for (index, (name, id)) in get_versions().iter().enumerate().rev() {
        let ident = Ident::new(&format!("Variant_{}_{}", main_ident.to_string(), name.replace('.', "_")), main_ident.span());
        let next_ident = if index == 0 {
            main_ident.clone()
        } else {
            Ident::new(&format!("Variant_{}_{}", main_ident.to_string(), get_versions()[index - 1].0.replace('.', "_")), main_ident.span())
        };
        deserialize_expr = quote!(
            #next_ident::try_from(if version >= #id {
                #ident::deserialize(deserializer)?
            } else {
                #deserialize_expr
            }).map_err(|e| serde::de::Error::custom(e.msg()))?
        );
    }
    let mut serialize_block = quote!(let ser = self;);
    for (name, id) in get_versions() {
        let ident = Ident::new(&format!("Variant_{}_{}", main_ident.to_string(), name.replace('.', "_")), main_ident.span());
        serialize_block.append_all(quote!(
            let ser = #ident::try_from(ser).map_err(|e| serde::ser::Error::custom(e.msg()))?;
            if version >= #id {
                return ser.serialize(serializer);
            }
        ));
    }
    serialize_block.append_all(quote!(
        return Err(serde::ser::Error::custom("version is older than supported"));
    ));

    output.append_all(quote!(
        impl<'de> crate::convert::VersionedSerde<'de> for #main_ident {
            #[allow(clippy::needless_question_mark)]
            fn deserialize<D>(version: u32, deserializer: D) -> std::result::Result<Self, D::Error>
            where D: serde::Deserializer<'de>
            {
                use serde::de::Deserialize;
                use std::convert::TryFrom;
                Ok(#deserialize_expr)
            }

            #[allow(clippy::needless_question_mark)]
            fn serialize<S>(self, version: u32, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where S: serde::Serializer
            {
                use serde::ser::Serialize;
                use std::convert::TryFrom;
                #serialize_block
            }
        }
    ));

    output.into()
}