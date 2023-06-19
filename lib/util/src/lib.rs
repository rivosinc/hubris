// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::any::type_name;
use std::fmt::LowerHex;
use std::str::FromStr;

use proc_macro2::{Literal, TokenStream, TokenTree};
use quote::TokenStreamExt;

/// Wrap an integer so it interpolates as a hexadecimal.
#[derive(Debug)]
pub struct Hex<T>(pub T);

impl<T: LowerHex> quote::ToTokens for Hex<T> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let lit =
            Literal::from_str(&format!("{:#x}{}", self.0, type_name::<T>()))
                .unwrap();
        TokenStreamExt::append(tokens, TokenTree::Literal(lit));
    }
}
