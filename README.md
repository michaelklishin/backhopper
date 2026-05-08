# backhopper

A Rust CLI that records the public API of Erlang/Elixir projects across all
their git tags into deterministic textual snapshots, then answers
compatibility questions against those snapshots.

Built to remove the manual research step gating RabbitMQ patch backports
across release branches and dependency versions (`ra`, `khepri`, `osiris`,
`cowboy`).

See [`design.md`](https://github.com/michaelklishin/backhopper/blob/main/design.md)
for the full design.

## Status

Pre-release. Phase 1 ships when initial discovery, snapshot I/O, and
existence queries are green.

## Quick start

```bash
backhopper snapshots discover --project ra
backhopper api lookup --project cowboy --tag 2.13.0 \
                     --mfa cowboy_req:set_resp_header/3
git format-patch -1 --stdout HEAD | \
  backhopper compatibility patch --series rabbitmq-4.2
```

## License

Dual-licensed under MIT or Apache-2.0, at your option.
