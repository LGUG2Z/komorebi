#![warn(clippy::all, clippy::nursery, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![no_implicit_prelude]

use ::std::clone::Clone;
use ::std::convert::From;
use ::std::convert::Into;
use ::std::format;
use ::std::iter::Extend;
use ::std::iter::Iterator;
use ::std::matches;
use ::std::option::Option::Some;
use ::std::string::String;
use ::std::string::ToString;
use ::std::unreachable;
use ::std::vec::Vec;

use ::quote::quote;
use ::syn::parse_macro_input;
use ::syn::Data;
use ::syn::DataEnum;
use ::syn::DeriveInput;
use ::syn::Fields;
use ::syn::FieldsNamed;
use ::syn::FieldsUnnamed;
use ::syn::Meta;
use ::syn::NestedMeta;

#[allow(clippy::too_many_lines)]
#[proc_macro_derive(AhkFunction)]
pub fn ahk_function(input: ::proc_macro::TokenStream) -> ::proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(s) => match s.fields {
            Fields::Named(FieldsNamed { named, .. }) => {
                let argument_idents = named
                    .iter()
                    // Filter out the flags
                    .filter(|&f| {
                        let mut include = true;
                        for attribute in &f.attrs {
                            if let ::std::result::Result::Ok(Meta::List(list)) =
                                attribute.parse_meta()
                            {
                                for nested in list.nested {
                                    if let NestedMeta::Meta(Meta::Path(path)) = nested {
                                        if path.is_ident("long") {
                                            include = false;
                                        }
                                    }
                                }
                            }
                        }

                        include
                    })
                    .map(|f| &f.ident);

                let argument_idents_clone = argument_idents.clone();

                let called_arguments = quote! {#(%#argument_idents_clone%) *}
                    .to_string()
                    .replace(" %", "%")
                    .replace("% ", "%")
                    .replace("%%", "% %");

                let flag_idents = named
                    .iter()
                    // Filter only the flags
                    .filter(|f| {
                        let mut include = false;

                        for attribute in &f.attrs {
                            if let ::std::result::Result::Ok(Meta::List(list)) =
                                attribute.parse_meta()
                            {
                                for nested in list.nested {
                                    if let NestedMeta::Meta(Meta::Path(path)) = nested {
                                        // Identify them using the --long flag name
                                        if path.is_ident("long") {
                                            include = true;
                                        }
                                    }
                                }
                            }
                        }

                        include
                    })
                    .map(|f| &f.ident);

                let has_flags = flag_idents.clone().count() != 0;

                if has_flags {
                    let flag_idents_concat = flag_idents.clone();
                    let argument_idents_concat = argument_idents.clone();

                    // Concat the args and flag args if there are flags
                    let all_arguments =
                        quote! {#(#argument_idents_concat,) * #(#flag_idents_concat), *}
                            .to_string();

                    let flag_idents_clone = flag_idents.clone();
                    let flags = quote! {#(--#flag_idents_clone) *}
                        .to_string()
                        .replace("- - ", "--")
                        .replace('_', "-");

                    let called_flag_arguments = quote! {#(%#flag_idents%) *}
                        .to_string()
                        .replace(" %", "%")
                        .replace("% ", "%")
                        .replace("%%", "% %");

                    let flags_split: Vec<_> = flags.split(' ').collect();
                    let flag_args_split: Vec<_> = called_flag_arguments.split(' ').collect();
                    let mut consolidated_flags: Vec<String> = Vec::new();

                    for (idx, flag) in flags_split.iter().enumerate() {
                        consolidated_flags.push(format!("{} {}", flag, flag_args_split[idx]));
                    }

                    let all_flags = consolidated_flags.join(" ");

                    quote! {
                        impl AhkFunction for #name {
                            fn generate_ahk_function() -> String {
                                ::std::format!(r#"
{}({}) {{
    RunWait, komorebic.exe {} {} {}, , Hide
}}"#,
                                    ::std::stringify!(#name),
                                    #all_arguments,
                                    ::std::stringify!(#name).to_kebab_case(),
                                    #called_arguments,
                                    #all_flags,
                                )
                           }
                        }
                    }
                } else {
                    let arguments = quote! {#(#argument_idents), *}.to_string();

                    quote! {
                        impl AhkFunction for #name {
                            fn generate_ahk_function() -> String {
                                ::std::format!(r#"
{}({}) {{
    RunWait, komorebic.exe {} {}, , Hide
}}"#, 
                                    ::std::stringify!(#name),
                                    #arguments,
                                    ::std::stringify!(#name).to_kebab_case(),
                                    #called_arguments
                                )
                           }
                        }
                    }
                }
            }
            _ => unreachable!("only to be used on structs with named fields"),
        },
        _ => unreachable!("only to be used on structs"),
    }
    .into()
}

#[proc_macro_derive(AhkLibrary)]
pub fn ahk_library(input: ::proc_macro::TokenStream) -> ::proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Enum(DataEnum { variants, .. }) => {
            let enums = variants.iter().filter(|&v| {
                matches!(v.fields, Fields::Unit) || matches!(v.fields, Fields::Unnamed(..))
            });

            let mut stream = ::proc_macro2::TokenStream::new();

            for variant in enums.clone() {
                match &variant.fields {
                    Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                        for field in unnamed {
                            stream.extend(quote! {
                                v.push(#field::generate_ahk_function());
                            });
                        }
                    }
                    Fields::Unit => {
                        let name = &variant.ident;
                        stream.extend(quote! {
                            v.push(::std::format!(r#"
{}() {{
    RunWait, komorebic.exe {}, , Hide
}}"#, 
                                ::std::stringify!(#name),
                                ::std::stringify!(#name).to_kebab_case()
                            ));
                        });
                    }
                    Fields::Named(_) => {
                        unreachable!("only to be used with unnamed and unit fields");
                    }
                }
            }

            quote! {
                impl #name {
                    fn generate_ahk_library() -> String {
                        let mut v: Vec<String> = vec![String::from("; Generated by komorebic.exe")];

                        #stream

                        v.join("\n")
                    }
               }
            }
        }
        _ => unreachable!("only to be used on enums"),
    }
    .into()
}
