//! This module defines a set of macros used to reference assets in Warp.
//!
//! The two types of assets are:
//! - Bundled: These are always included in the app bundle. These files are located in `app/assets/bundled`.
//!   Access with `bundled_asset!([path of asset relative to app/assets/bundled])`.
//! - Async: Larger assets that standalone CLI builds leave out of the bundle. These files live in
//!   `app/assets/async`. Access with
//!   `bundled_or_fetched!(path of asset relative to app/assets/async])`.
//!
//! These macros check for the existence of the asset at the appropriate location before returning
//! an `AssetSource` with the appropriate bundle reference.
//!
//! You can specify a specific folder under `app/assets` to look in as the second argument to any
//! of these macros, but you probably shouldn't be doing that.

#![recursion_limit = "1024"]
#[macro_use]
extern crate quote;
extern crate proc_macro;

use std::env;
use std::path::PathBuf;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::parse::Parse;
use syn::{LitStr, Token, parse_macro_input};
use warp_util::assets::{ASSETS_DIR, ASYNC_ASSETS_DIR, BUNDLED_ASSETS_DIR};

struct MacroArgs {
    /// The name of the asset. E.g. `jpg/jellyfish_bg.jpg`
    asset_name: LitStr,
    /// The asset subfolder under `app/assets`. E.g. `async`.
    asset_folder: Option<LitStr>,
}

impl Parse for MacroArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse either one string literal (the asset location) or two comma-separated string
        // literals (the asset location and the asset subfolder).
        Ok(MacroArgs {
            asset_name: input.parse()?,
            asset_folder: if input.peek(Token![,]) {
                let _comma: Token![,] = input.parse()?;
                Some(input.parse()?)
            } else {
                None
            },
        })
    }
}

#[proc_macro]
pub fn bundled_asset(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as MacroArgs);
    let asset_name = args.asset_name.value();
    let asset_folder_arg = args.asset_folder.map(|s| s.value());
    let asset_folder = asset_folder_arg.as_deref().unwrap_or(BUNDLED_ASSETS_DIR);

    match construct_bundled_asset(&asset_name, asset_folder) {
        Ok(ok) => ok.into(),
        Err(err_str) => format_error(&asset_name, asset_folder, err_str).into(),
    }
}

fn construct_bundled_asset(asset_name: &str, asset_dir: &str) -> Result<TokenStream2, String> {
    if full_asset_path(asset_name, asset_dir).exists() {
        let full_location = format!("{asset_dir}/{asset_name}");
        Ok(quote! {
            ::warpui_core::assets::asset_cache::AssetSource::Bundled {
                path: #full_location .into(),
            }
        })
    } else {
        Err("file not found".into())
    }
}

#[proc_macro]
pub fn bundled_or_fetched_asset(input: TokenStream) -> TokenStream {
    let input_lit = parse_macro_input!(input as LitStr);

    quote! {
        ::asset_macro::bundled_asset!( #input_lit, #ASYNC_ASSETS_DIR )
    }
    .into()
}

fn full_asset_path(asset_name: &str, asset_dir: &str) -> PathBuf {
    // The working directory when running a proc macro is not guaranteed, so we base relative paths
    // off the location of the cargo manifest.
    let crate_root =
        env::var("CARGO_MANIFEST_DIR").expect("missing basic cargo environment variable");

    PathBuf::from(crate_root)
        .join(ASSETS_DIR)
        .join(asset_dir)
        .join(asset_name)
}

fn format_error(asset_name: &str, asset_dir: &str, error_string: String) -> TokenStream2 {
    let full_path = full_asset_path(asset_name, asset_dir);
    let error_message = format!("Error loading asset at {full_path:?}: {error_string}");

    quote! {
        compile_error!(#error_message)
    }
}
