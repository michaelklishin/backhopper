# backhopper-cuttlefish

Cuttlefish `.schema` extractor for [backhopper](https://github.com/michaelklishin/backhopper).

`.schema` files in the RabbitMQ ecosystem are written in
[Cuttlefish](https://github.com/Kyorai/cuttlefish), a small Erlang DSL that
declares config-key mappings, validators, and translations:

```erlang
{translation, "rabbitmq.foo",
 fun(Conf) ->
     rabbit_cuttlefish:optionally_tagged_string("rabbitmq.foo", Conf)
 end}.
```

This crate finds the top-level `translation`, `validator`, and `mapping`
tuples in a `.schema` source, isolates the embedded `fun(...) -> Body end`
bodies, and reports their source location so the backhopper Erlang
call-site extractor can flag references to host-repo functions like
`rabbit_cuttlefish:optionally_tagged_string/2`.

## What it parses

| Tuple form | Treatment |
|---|---|
| `{translation, "<key>", fun(C) -> Body end}` | Body extracted, source-line tagged |
| `{validator, "<name>", "<msg>", fun(V) -> Body end}` | Body extracted, source-line tagged |
| `{mapping, "<key>", "<conf-path>", [...]}` | Path-only signal; no Erlang body |

## What it does NOT parse

This crate has nothing to do with `rabbitmq.conf` (the ini-like user-facing
input). For that, see [`rabbitmq-conf`](https://crates.io/crates/rabbitmq-conf).

## License

Double licensed under the MIT and Apache 2.0 (ASL2) licenses.

See `LICENSE-APACHE` and `LICENSE-MIT` for details.

SPDX-License-Identifier: Apache-2.0 OR MIT

## Copyright

(c) 2026 Michael S. Klishin and Contributors.
