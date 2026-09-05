// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use proc_macro::{Group, Ident, TokenStream, TokenTree};

// default is all false
#[derive(Default)]
struct Flags {
    flag_with: bool,
    flag_except: bool,
    flag_at: bool,
}

#[derive(Default)]
struct ParserBuilder {
    len: usize,
    default: TokenStream,
    except: TokenStream,
    except_index: usize,
}

impl ParserBuilder {
    fn build(self) -> TupleParser {
        TupleParser {
            len: self.len,
            default: self.default,
            except: self.except,
            except_index: self.except_index,
        }
    }
}

// returns true if needs to handle default or except
fn parse_ident(ident: &Ident, idx: &mut usize, flags: &mut Flags) -> bool {
    // Get Separator "with/except/at"
    match ident.to_string().as_str() {
        "with" => {
            flags.flag_with = true;
            *idx += 1;
            false
        }
        "except" => {
            flags.flag_except = true;
            *idx += 1;
            false
        }
        "at" => {
            flags.flag_at = true;
            *idx += 1;
            false
        }
        _ => true,
    }
}

// returns true if needs to handle default or except
fn parse_group(
    group: &Group,
    idx: &mut usize,
    flags: &mut Flags,
    builder: &mut ParserBuilder,
) -> bool {
    if !flags.flag_with && !flags.flag_except && !flags.flag_at {
        // The tuple length is obtained by calculating the number of parenthesis layers
        // of ((0 + 1) + 1). Actually the '0' also has a parenthesis wrapped around it,
        // So here have to -1.
        builder.len = group_num(group.stream()) - 1;
        *idx += 1;
        return false;
    }
    if flags.flag_with && flags.flag_except && flags.flag_at {
        // Get the except_index.
        builder.except_index = group.stream().into_iter().count();
        *idx += 1;
        return false;
    }
    true
}

fn parse_token(buf: &[TokenTree], idx: &mut usize, flags: &mut Flags, builder: &mut ParserBuilder) {
    match &buf[*idx] {
        TokenTree::Ident(ident) => {
            if !parse_ident(ident, idx, flags) {
                return;
            }
        }
        TokenTree::Group(group) => {
            if !parse_group(group, idx, flags, builder) {
                return;
            }
        }
        _ => {}
    }
    // Get TupleParser's 'default' or 'except'
    if flags.flag_with && !flags.flag_at {
        let default_or_except = TokenStream::from((buf[*idx]).to_owned());
        if flags.flag_except {
            builder.except.extend(default_or_except);
        } else {
            builder.default.extend(default_or_except);
        }
    }

    *idx += 1;
}

/// Convert [`TokenStream`] to [`TupleParser`]
pub(crate) fn tuple_parser(input: TokenStream) -> TupleParser {
    let buf = input.into_iter().collect::<Vec<_>>();
    let mut flags = Flags::default();
    let mut builder = ParserBuilder::default();
    let mut idx = 0;

    while idx < buf.len() {
        parse_token(&buf, &mut idx, &mut flags, &mut builder);
    }
    builder.build()
}

/// Recursively queried how many layers of [`TokenTree::Group`]
fn group_num(inner: TokenStream) -> usize {
    match inner.into_iter().next() {
        Some(TokenTree::Group(group)) => group_num(group.stream()) + 1,
        _ => 0,
    }
}

/// Necessary data for building tuples
pub(crate) struct TupleParser {
    /// Length of Tuple
    pub(crate) len: usize,
    /// Default value of Tuple
    pub(crate) default: TokenStream,
    /// Except value of Tuple
    pub(crate) except: TokenStream,
    /// Index of the default value in the tuple
    pub(crate) except_index: usize,
}
