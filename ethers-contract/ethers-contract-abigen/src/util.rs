use ethers_core::types::Address;

use anyhow::{anyhow, Result};
use cargo_metadata::{DependencyKind, MetadataCommand};
use inflector::Inflector;
use once_cell::sync::Lazy;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;

use syn::{Ident as SynIdent, Path};

/// See `determine_ethers_crates`
///
/// This ensures that the `MetadataCommand` is only run once
static ETHERS_CRATES: Lazy<(&'static str, &'static str, &'static str)> =
    Lazy::new(determine_ethers_crates);

/// Convenience function to turn the `ethers_core` name in `ETHERS_CRATE` into a `Path`
pub fn ethers_core_crate() -> Path {
    syn::parse_str(ETHERS_CRATES.0).expect("valid path; qed")
}
/// Convenience function to turn the `ethers_contract` name in `ETHERS_CRATE` into an `Path`
pub fn ethers_contract_crate() -> Path {
    syn::parse_str(ETHERS_CRATES.1).expect("valid path; qed")
}
pub fn ethers_providers_crate() -> Path {
    syn::parse_str(ETHERS_CRATES.2).expect("valid path; qed")
}

/// The crates name to use when deriving macros: (`core`, `contract`)
///
/// We try to determine which crate ident to use based on the dependencies of
/// the project in which the macro is used. This is useful because the macros,
/// like `EthEvent` are provided by the `ethers-contract` crate which depends on
/// `ethers_core`. Most commonly `ethers` will be used as dependency which
/// reexports all the different crates, essentially `ethers::core` is
/// `ethers_core` So depending on the dependency used `ethers` ors `ethers_core
/// | ethers_contract`, we need to use the fitting crate ident when expand the
/// macros This will attempt to parse the current `Cargo.toml` and check the
/// ethers related dependencies.
///
/// This process is a bit hacky, we run `cargo metadata` internally which
/// resolves the current package but creates a new `Cargo.lock` file in the
/// process. This is not a problem for regular workspaces but becomes an issue
/// during publishing with `cargo publish` if the project does not ignore
/// `Cargo.lock` in `.gitignore`, because then cargo can't proceed with
/// publishing the crate because the created `Cargo.lock` leads to a modified
/// workspace, not the `CARGO_MANIFEST_DIR` but the workspace `cargo publish`
/// created in `./target/package/..`. Therefore we check prior to executing
/// `cargo metadata` if a `Cargo.lock` file exists and delete it afterwards if
/// it was created by `cargo metadata`.
pub fn determine_ethers_crates() -> (&'static str, &'static str, &'static str) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("No Manifest found");

    // check if the lock file exists, if it's missing we need to clean up afterward
    let lock_file = format!("{}/Cargo.lock", manifest_dir);
    let needs_lock_file_cleanup = !std::path::Path::new(&lock_file).exists();

    let res = MetadataCommand::new()
        .manifest_path(&format!("{}/Cargo.toml", manifest_dir))
        .exec()
        .ok()
        .and_then(|metadata| {
            metadata.root_package().and_then(|pkg| {
                pkg.dependencies.iter().filter(|dep| dep.kind == DependencyKind::Normal).find_map(
                    |dep| {
                        (dep.name == "ethers")
                            .then(|| ("ethers::core", "ethers::contract", "ethers::providers"))
                    },
                )
            })
        })
        .unwrap_or(("ethers_core", "ethers_contract", "ethers_providers"));

    if needs_lock_file_cleanup {
        // delete the `Cargo.lock` file that was created by `cargo metadata`
        // if the package is not part of a workspace
        let _ = std::fs::remove_file(lock_file);
    }

    res
}

/// Expands a identifier string into an token.
pub fn ident(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

/// Expands an identifier string into a token and appending `_` if the
/// identifier is for a reserved keyword.
///
/// Parsing keywords like `self` can fail, in this case we add an underscore.
pub fn safe_ident(name: &str) -> Ident {
    syn::parse_str::<SynIdent>(name).unwrap_or_else(|_| ident(&format!("{}_", name)))
}

/// Reapplies leading and trailing underscore chars to the ident
/// Example `ident = "pascalCase"; alias = __pascalcase__` -> `__pascalCase__`
pub fn preserve_underscore_delim(ident: &str, alias: &str) -> String {
    alias
        .chars()
        .take_while(|c| *c == '_')
        .chain(ident.chars())
        .chain(alias.chars().rev().take_while(|c| *c == '_'))
        .collect()
}

/// Expands a positional identifier string that may be empty.
///
/// Note that this expands the parameter name with `safe_ident`, meaning that
/// identifiers that are reserved keywords get `_` appended to them.
pub fn expand_input_name(index: usize, name: &str) -> TokenStream {
    let name_str = match name {
        "" => format!("p{}", index),
        n => n.to_snake_case(),
    };
    let name = safe_ident(&name_str);

    quote! { #name }
}

/// Expands a doc string into an attribute token stream.
pub fn expand_doc(s: &str) -> TokenStream {
    let doc = Literal::string(s);
    quote! {
        #[doc = #doc]
    }
}

pub fn expand_derives(derives: &[Path]) -> TokenStream {
    quote! {#(#derives),*}
}

/// Parses the given address string
pub fn parse_address<S>(address_str: S) -> Result<Address>
where
    S: AsRef<str>,
{
    let address_str = address_str.as_ref();
    if !address_str.starts_with("0x") {
        return Err(anyhow!("address must start with '0x'"))
    }
    Ok(address_str[2..].parse()?)
}

#[cfg(not(target_arch = "wasm32"))]
/// Perform an HTTP GET request and return the contents of the response.
pub fn http_get(url: &str) -> Result<String> {
    Ok(reqwest::blocking::get(url)?.text()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_name_to_ident_empty() {
        assert_quote!(expand_input_name(0, ""), { p0 });
    }

    #[test]
    fn input_name_to_ident_keyword() {
        assert_quote!(expand_input_name(0, "self"), { self_ });
    }

    #[test]
    fn input_name_to_ident_snake_case() {
        assert_quote!(expand_input_name(0, "CamelCase1"), { camel_case_1 });
    }

    #[test]
    fn parse_address_missing_prefix() {
        assert!(
            !parse_address("0000000000000000000000000000000000000000").is_ok(),
            "parsing address not starting with 0x should fail"
        );
    }

    #[test]
    fn parse_address_address_too_short() {
        assert!(
            !parse_address("0x00000000000000").is_ok(),
            "parsing address not starting with 0x should fail"
        );
    }

    #[test]
    fn parse_address_ok() {
        let expected =
            Address::from([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
        assert_eq!(parse_address("0x000102030405060708090a0b0c0d0e0f10111213").unwrap(), expected);
    }
}
