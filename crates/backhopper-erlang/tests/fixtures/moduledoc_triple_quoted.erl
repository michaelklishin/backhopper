%% Copyright (C) 2026 Michael S. Klishin and Contributors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%% See LICENSE-APACHE and LICENSE-MIT for details.
%%
%% Distilled from the OTP 27 documentation shapes that wiped the export
%% lists of crypto, code, edlin, and merl: markdown bullets at column
%% zero, a fenced example holding an Erlang call, an attribute-shaped
%% line inside prose, and unbalanced quotes.

-module(moduledoc_triple_quoted).

-moduledoc """
Functions for cryptography and key derivation.

- **`strong_rand_bytes/1`** - random bytes from the "strong entropy source.
- **`mac/4`** - a message authentication code.

An `-export([phantom/0]).` line here is prose, not an attribute.

```erlang
1> crypto:strong_rand_bytes(16).
<<"..."">>
```
""".

-export([strong_rand_bytes/1, mac/4]).
-export([supports/1]).

-doc """
Returns `N` random bytes. The generator is seeded once per scheduler.
""".
-spec strong_rand_bytes(non_neg_integer()) -> binary().
strong_rand_bytes(N) ->
    crypto:strong_rand_bytes(N).

-doc "Computes a message authentication code.".
-spec mac(atom(), atom(), binary(), iodata()) -> binary().
mac(Type, SubType, Key, Data) ->
    crypto:mac(Type, SubType, Key, Data).

-doc false.
-spec supports(atom()) -> [atom()].
supports(Category) ->
    crypto:supports(Category).
