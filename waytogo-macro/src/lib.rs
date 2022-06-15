mod hexde;
mod macroimpl;
use macroimpl::generate;
use syn::{parse_macro_input, LitStr};

#[proc_macro]
pub fn wayland_protocol(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let input = lit.value();
    generate(input).into()
}

#[cfg(test)]
mod test {
    use rust_format::{Config, Formatter, PostProcess, RustFmt};

    use crate::macroimpl::generate;

    #[test]
    fn test_xdg_shell() {
        println!(
            "{}",
            test("/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml".to_string())
        )
    }

    #[test]
    fn test_wayland() {
        println!("{}", test("/usr/share/wayland/wayland.xml".to_string()))
    }

    fn test(loc: String) -> String {
        let source = generate(loc);
        let config = Config::new_str().post_proc(PostProcess::ReplaceMarkersAndDocBlocks);
        RustFmt::from_config(config).format_tokens(source).unwrap()
    }
}
