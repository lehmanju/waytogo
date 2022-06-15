use crate::hexde::HexDe;
use proc_macro2::{Span, TokenStream};
use regex::Regex;
use std::{
    env,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use syn::Token;
use syn::{parse_macro_input, token::Token};

use heck::{ToSnakeCase, ToUpperCamelCase};

use quick_xml::{de::from_reader, Reader};
use quote::{format_ident, quote, quote_spanned, ToTokens, TokenStreamExt};
use serde::Deserialize;
use syn::LitStr;

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Protocol {
    pub(crate) name: String,
    #[serde(rename = "$value")]
    pub(crate) interfaces: Vec<ProtocolEnum>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) enum ProtocolEnum {
    #[serde(rename = "interface")]
    Interface(Interface),
    #[serde(rename = "copyright")]
    Copyright(Copyright),
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Copyright {
    #[serde(rename = "$value")]
    pub(crate) content: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Interface {
    name: String,
    #[serde(rename = "$value", default)]
    fields: Vec<InterfaceEnum>,
    version: u32,
}

#[derive(Debug, Deserialize, PartialEq)]

pub(crate) enum InterfaceEnum {
    #[serde(rename = "request")]
    Request(Request),
    #[serde(rename = "event")]
    Event(Event),
    #[serde(rename = "enum")]
    Enum(Enum),
    #[serde(rename = "description")]
    Description(Description),
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Description {
    summary: String,
    #[serde(rename = "$value")]
    body: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Enum {
    name: String,
    description: Option<Description>,
    bitfield: Option<bool>,
    #[serde(rename = "entry")]
    entries: Vec<Entry>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Entry {
    name: String,
    #[serde(with = "HexDe")]
    value: u32,
    summary: Option<String>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Request {
    name: String,
    description: Description,
    #[serde(rename = "arg", default)]
    args: Vec<Arg>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Event {
    name: String,
    description: Option<Description>,
    since: Option<u32>,
    #[serde(rename = "arg", default)]
    args: Vec<Arg>,
}
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Arg {
    name: String,
    #[serde(rename = "type")]
    t: ArgType,
    #[serde(rename = "enum")]
    enumt: Option<String>,
    interface: Option<String>,
    summary: Option<String>,
    allow_null: Option<bool>,
}

#[derive(Debug, Deserialize, PartialEq)]

pub(crate) enum ArgType {
    #[serde(rename = "uint")]
    Uint,
    #[serde(rename = "fd")]
    Fd,
    #[serde(rename = "int")]
    Int,
    #[serde(rename = "string")]
    String,
    #[serde(rename = "object")]
    Object,
    #[serde(rename = "new_id")]
    NewId,
    #[serde(rename = "array")]
    Array,
    #[serde(rename = "fixed")]
    Fixed,
}

impl ToTokens for ArgType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            ArgType::Uint => tokens.append(format_ident!("{}", "u32")),
            ArgType::Fd => tokens.append(format_ident!("{}", "RawFd")),
            ArgType::Int => tokens.append(format_ident!("{}", "i32")),
            ArgType::String => {
                tokens.append(format_ident!("{}", "Box"));
                tokens.append_all(quote!(<));
                tokens.append(format_ident!("{}", "CString"));
                tokens.append_all(quote!(>));
            }
            ArgType::Object => tokens.append(format_ident!("{}", "LookupId")),
            ArgType::NewId => tokens.append(format_ident!("{}", "NewId")),
            ArgType::Array => {
                tokens.append(format_ident!("{}", "Box"));
                tokens.append_all(quote!(<));
                tokens.append(format_ident!("{}", "Vec"));
                tokens.append_all(quote!(<));
                tokens.append(format_ident!("{}", "u8"));
                tokens.append_all(quote!(>));
                tokens.append_all(quote!(>));
            }
            ArgType::Fixed => tokens.append(format_ident!("{}", "i32")),
        }
    }
}

impl ArgType {
    fn to_argument(&self) -> &'static str {
        match self {
            ArgType::Uint => "Uint",
            ArgType::Fd => "Fd",
            ArgType::Int => "Int",
            ArgType::String => "Str",
            ArgType::Object => "Object",
            ArgType::NewId => "NewId",
            ArgType::Array => "Array",
            ArgType::Fixed => "Fixed",
        }
    }
}

pub(crate) fn generate(input: String) -> TokenStream {
    let f = File::open(input).unwrap();
    let reader = BufReader::new(f);
    let protocol: Protocol = from_reader(reader).unwrap();
    dbg!(&protocol);

    let mut interfaces = TokenStream::new();

    for int in protocol.interfaces {
        match int {
            ProtocolEnum::Interface(int) => {
                let int_snake = format_ident!("{}", int.name.to_snake_case());
                let int_camel = format_ident!("{}", int.name.to_upper_camel_case());
                let mut request_opcode = 0u16;
                let mut event_opcode = 0u16;
                let mut interface_tks = TokenStream::new();
                let mut description = None;
                let mut interface_use = TokenStream::new();
                for field in int.fields {
                    match field {
                        InterfaceEnum::Request(request) => {
                            let req_name = format_ident!("{}", request.name.to_upper_camel_case());
                            let req_desc = format!(
                                "{}\n\n{}",
                                request.description.summary,
                                &request
                                    .description
                                    .body
                                    .unwrap_or_default()
                                    .replace("\t", "")
                            );
                            let args = request.args;
                            let mut args_struct = TokenStream::new();
                            let mut args_argument = TokenStream::new();
                            let mut return_interface = None;
                            let num_args = args.len();
                            for mut arg in args {
                                if matches!(arg.t, ArgType::NewId) {
                                    if let Some(return_type) = arg.interface.take() {
                                        if return_interface.is_some() {
                                            //warn
                                        }
                                        return_interface = Some((
                                            format_ident!("{}", arg.name.clone()),
                                            return_type,
                                        ));
                                    } else {
                                        // not implemented
                                    }
                                } else if matches!(arg.t, ArgType::Object) {
                                    // ignore interface for now
                                }
                                let arg_name = format_ident!("{}", arg.name);
                                let arg_doc = arg.summary.unwrap_or(String::new());
                                let arg_t = &arg.t;
                                let arg_variant = format_ident!("{}", arg.t.to_argument());
                                let tk_arg_struct = quote! {
                                    #[doc = #arg_doc]
                                    pub #arg_name: #arg_t,
                                };
                                let tk_args_argument = quote! {
                                    Argument::#arg_variant(self.#arg_name),
                                };
                                args_struct.extend(tk_arg_struct);
                                args_argument.extend(tk_args_argument);
                            }

                            let tks_impl;
                            let tks_use;

                            if let Some((arg_name, arg_interface)) = return_interface {
                                let return_type =
                                    format_ident!("{}", arg_interface.to_upper_camel_case());
                                let return_snake =
                                    format_ident!("{}", arg_interface.to_snake_case());
                                tks_use = quote! {
                                    use super::#return_snake;
                                };
                                tks_impl = quote! {
                                    impl RequestObject for #req_name {
                                        type Interface = #int_camel;
                                        type ReturnType = #return_snake::#return_type;

                                        fn apply(self, self_id: Id, _interface: &mut Self::Interface) -> (Self::ReturnType, Message) {
                                            let args = vec![#args_argument];
                                            let message = Message {
                                                sender_id: self_id,
                                                opcode: #request_opcode,
                                                args,
                                            };
                                            (
                                                #return_snake::#return_type {},
                                                message,
                                            )
                                        }

                                        fn id(&self) -> &NewId {
                                            &self.#arg_name
                                        }
                                    }
                                }
                            } else {
                                tks_use = TokenStream::new();
                                tks_impl = quote! {
                                    impl Request for #req_name {
                                        type Interface = #int_camel;

                                        fn apply(self, self_id: Id, _interface: &mut Self::Interface) -> Message {
                                            let args = vec![#args_argument];
                                            Message {
                                                sender_id: self_id,
                                                opcode: #request_opcode,
                                                args,
                                            }
                                        }
                                    }
                                }
                            }

                            let tks = quote! {
                                #tks_use

                                #[doc = #req_desc]
                                pub struct #req_name {
                                    #args_struct
                                }

                                #tks_impl
                            };
                            interface_tks.extend(tks);
                            request_opcode += 1;
                        }
                        InterfaceEnum::Event(event) => {
                            event_opcode += 1;
                        }
                        InterfaceEnum::Enum(en) => {}
                        InterfaceEnum::Description(desc) => description = Some(desc),
                    }
                }
                let desc_tks = if let Some(desc) = description {
                    format!(
                        "{}\n\n{}",
                        desc.summary,
                        Regex::new(r"\n\n\s+")
                            .unwrap()
                            .replace_all(&desc.body.unwrap_or_default().replace("\t", ""), "\n\n")
                    )
                } else {
                    String::new()
                };
                let tks = quote! {
                    #[doc = #desc_tks]
                    pub mod #int_snake {


                        use std::ffi::CString;
                        use std::os::unix::prelude::RawFd;
                        use super::Id;
                        use super::LookupId;
                        use super::Argument;
                        use super::Request;
                        use super::RequestObject;
                        use super::Message;
                        use super::WaylandInterface;
                        use super::NewId;
                        use super::Signature;

                        #interface_use
                        #[doc = #desc_tks]
                        pub struct #int_camel;

                        impl WaylandInterface for #int_camel {
                            fn event_signature() -> Signature {
                                todo!()
                            }

                            fn request_signature() -> Signature {
                                todo!()
                            }

                            fn interface() -> &'static str {
                                todo!()
                            }

                            fn version() -> u32 {
                                todo!()
                            }
                        }

                        #interface_tks
                    }
                };
                interfaces.extend(tks);
            }
            ProtocolEnum::Copyright(copyright) => {}
        }
    }

    return interfaces;
}
