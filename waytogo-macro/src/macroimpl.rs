use crate::hexde::HexDe;
use proc_macro2::TokenStream;
use std::{
    env,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use syn::parse_macro_input;

use heck::ToSnakeCase;

use quick_xml::{de::from_reader, Reader};
use quote::quote;
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

impl ArgType {
    fn to_string(&self) -> &'static str {
        match self {
            ArgType::Uint => "u32",
            ArgType::Fd => "RawFd",
            ArgType::Int => "i32",
            ArgType::String => "Box<CString>",
            ArgType::Object => "u32",
            ArgType::NewId => "NewId",
            ArgType::Array => "Vec<u8>",
            ArgType::Fixed => "todo!()",
        }
    }
}

pub(crate) fn generate(input: String) -> TokenStream {
    let f = File::open(input).unwrap();
    let reader = BufReader::new(f);
    let protocol: Protocol = from_reader(reader).unwrap();
    dbg!(protocol);

    //let mut interfaces = Vec::new();

 /*    for int in protocol.interfaces {
        let int_lower = int.name.to_snake_case();
        let int_name = int.name;
        //let mut events = Vec::new();
        //let mut requests = Vec::new();
        //let mut enums = Vec::new();
        let mut description = None;
        for field in int.fields {
            match field {
                InterfaceEnum::Request(request) => {
                    let req_name = request.name;
                    let req_desc = request.description.summary + ". " + &request.description.body;
                    let args = request.args;
                    let mut args_tk = TokenStream::new();
                    let mut arg_interface = None;
                    for arg in args {
                        arg_interface = arg.interface;
                        let arg_name = arg.name;
                        let arg_doc = arg.summary.unwrap_or(String::new());
                        let arg_t = arg.t.to_string();
                        let tk = quote! {
                            #[doc = #arg_doc]
                            pub #arg_name: #arg_t,
                        };
                        args_tk.extend(tk);
                    }

                    let tks = quote! {
                        #[doc = #req_desc]
                        pub struct #req_name {
                            #args_tk
                        }

                        impl RequestObject for #req_name {
                            type Interface = #int_name;
                            type ReturnType = WlRegistry;

                            fn apply(self, self_id: Id, _interface: &mut Self::Interface) -> (Self::ReturnType, Message) {
                                let args = smallvec![Argument::NewId(self.registry)];
                                let message = Message {
                                    sender_id: self_id,
                                    opcode: 1,
                                    args,
                                };
                                (
                                    WlRegistry {
                                        name_id_map: HashMap::new(),
                                        available_names: HashSet::new(),
                                    },
                                    message,
                                )
                            }

                            fn id(&self) -> &NewId {
                                &self.registry
                            }
                        }
                    };
                }
                InterfaceEnum::Event(event) => {}
                InterfaceEnum::Enum(en) => {}
                InterfaceEnum::Description(desc) => description = Some(desc),
            }
        }
        let tks = quote! {
            mod #int_lower {
                pub struct #int_name;


            }
        };
        interfaces.push(tks)
    }
 */
    todo!()
}
