mod macroimpl;
mod hexde;
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
    use crate::macroimpl::generate;

    #[test]
    fn test_xdg_shell() {
        println!(
            "{}",
            generate("/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml".to_string()).to_string()
        )
    }

    #[test]
    fn test_wayland() {
        println!(
            "{}",
            generate("/usr/share/wayland/wayland.xml".to_string()).to_string()
        )
    }
}
