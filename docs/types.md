# Types Catalogue

The shapes this tree uses to hold a rule in a type instead of in a
comment or a check. One entry per shape: what it guarantees, when to
use it, when not to, and the types that already use it. The rule
itself is in the "Types Over Comments" section of `AGENTS.md`;
constitution article `0003` is the argument.

Before adding a check or a comment for a new rule, find the rule's
shape below. Usually a type here already holds it.

## Validated name

A string with a domain meaning gets its own type, and the constructor
refuses anything outside its charset. `string_newtype!` generates
`Display`, `FromStr` and `serde(transparent)` from one declaration.

Use for a name with a fixed charset: `ModuleName`, `TagName`,
`ProjectName`, `CommitSha`.

Not for a pattern or free text. `TagGlob` is its own type, not a
`TagName`, because a pattern is not a name.

The kinds implement `Borrow<str>`, so a set of names answers
`contains` for a scanned `&str`. None implements `Deref` or
`From<String>`: that is what stops a call site from erasing the type.
All of them are in `model/names.rs`.

## Bounded scalar

When the carrier type already holds the bound, there is nothing to
check: `Arity(u8)` cannot exceed `255`.

Not for a bound narrower than the carrier. That needs a validating
constructor, not a bare wrapper.

## Fixed-width digest

A value that is always exactly N characters of one alphabet is a
newtype over that shape, not a `String` with a length comment.

`CommitSha` (forty lowercase hex characters), `CommitShaPrefix` (a
prefix of one), `VerdictFingerprint` (thirty-two, a truncated BLAKE3
digest).

## Composite key

A type built from already-validated parts, with no rule across them,
can be built any way: every way produces a valid value. `Mfa` has
three validated parts and is built by `new` or by the struct literal.

Not for a type with a rule across its fields. `Ratio` keeps
`hits <= total`, so `hit` and `miss` are its only public doors.

## One-way pipeline

A value that passes through a fixed sequence of states is generic over
a state marker, and each step adds methods. There is no `state` field
to rewind and no method on an earlier state that belongs to a later
one. The marker trait is sealed, so a foreign marker cannot step
outside the sequence.

`Patch<Raw>` to `Patch<Analyzed>`; `EvaluationContext<Pinned>` to
`<Scoped>` to `<Sourced>`; `TestSuiteFile<Raw>` to `<Parsed>` to
`<Resolved>`; `CallGraph<M, Building>` to `<M, Built>`.

Not for facts that can hold in any combination. Those are a struct of
flags, not a pipeline.

## Read-only handle

A handle that only reads has no write method, rather than a write
method with a check that refuses. `SnapshotStore<ReadOnly>` has no
`write`; `SnapshotStore<Mutable>` does. `StoreMode` is sealed, so no
third mode can add one.

## Required argument as a type parameter

A builder records "required argument supplied" in a type parameter, so
`run` before the argument is set does not compile. The driver's
`CheckPatchBuilder` and `CheckPositionalBuilder` do this over
`NoTarget` and `WithTarget`, `NoInput` and `WithInput`.

Not for an optional argument. An `Option` on the builder is enough.

## Resolved from a spec

What the config says and what it resolves to are two types, so a
function that needs the resolved value cannot be handed the spec.
`PinSpec` resolves to `Pin` through `PinSpec::resolve`, the one method
that consults the store. `PinSpec::as_self_pin` returns a `SelfPin`,
so the function that resolves a self-ref pin takes that arm and never
sees the other two.

## Identity choice

A value that is one of several kinds, never a mix, is an enum with one
variant per kind. Fields that only some kinds use live in their
variant, not beside a discriminant.

`PinSpec::{Literal, Pattern, SelfRef}`; `ProjectSource::{External {
git_url }, SelfRepo}`, so a self-project cannot carry a URL;
`PinSelector::{Series, Pin}`, so a check is addressed one way or the
other. `OptionKeySet::{Closed(keys), Open(keys), Unresolved}` for an
option-map type's parsed key universe, so a firing rule that must
consume only the closed form takes that arm and never has to check a
flag first. `GenerateAction::{Skip, Build, Refresh}` for what
`snapshots generate` does with one tag; its `as_write` narrows to
`WriteKind::{Build, Refresh}` once `Skip` is ruled out, so the
compiler drops the redundant `Skip` arm from every caller that already
handled it instead of leaving an `unreachable!`.

Use when two fields are set together, refused together, or mutually
exclusive. Fields that vary independently are a plain struct.

## Verdict with ground

A verdict that is not `Compatible` carries its reasons in the variant,
so a reader cannot have the answer without its ground:
`Verdict::{RequiresAdaptation { reasons }, Incompatible { reasons },
Inapplicable { reason }}`. The same shape: `RoundClearance` with
`ClearanceFacts` in every arm, `ModuleProvenance::FirstParty { path }`,
`BuildOutcome::CompilationFailed { class }`,
`ExtractorFreshness::{Stale { stored }, Unversioned { format_version
}}` and `doctor`'s `SnapshotStatus::{Stale { stored, expected },
Unversioned { format_version, expected }}`, so an unrecorded extractor
version carries the one fact it does have instead of collapsing into
`Stale` with an empty `stored`.

Not when the justification is about something else, such as the whole
run. `AggregateVerdict` is a label beside its facts for that reason.

## Three-state answer

Some questions have three answers: yes, no, and cannot say, with a
reason for the third. A `bool` or an `Option<bool>` cannot hold that
without a comment saying what the missing case means.

`Surface<T>::lookup` answers `Present`, `Absent` or
`Unknowable(Unreadable)` for an export surface, and has no `contains`
to mistake for a `bool`. `ExportedTypes` is `Listed` or `Unreadable`.
`IncludeCoverage` names which side of an include walk went unread, and
its two methods say what that can hide. `EvaluationFiles` is a
`BTreeMap<PathBuf, Option<Vec<u8>>>`: a missing key, `None` and bytes
are three readings.

Not when the third state is "not computed yet" with no ground of its
own. `Option<T>` already says that.

## Versioned wire

A value that crosses a process boundary is versioned, and a wire
struct's shape is history: it keeps its flat `Option` fields forever.
The type that cannot disagree with itself lives one layer in, in one
of two shapes:

* private fields with one setter, when the process that serialises
  the value sets them together: `TargetAxisSlot` holds `apply` and
  `target_findings`, written only by `present`
* a derived enum with one arm per producer generation, when the fact
  is about who wrote the value: `Producer::{BeforeV12, Current}` reads
  `self_projects`, `resolver_coverage` and `fingerprint_version` as
  one answer

The schema table is one row per version in ascending order, with a
`const` assertion on the length and on each row's position, so a
missing, duplicated or misplaced row is a compile error. The snapshot
format has `FORMAT_VERSION` and `SUPPORTED_FORMAT_VERSIONS`.
`SnapshotHeader.format_version` retains the value the parser actually
read, distinct from the constant a fresh write always emits: reading
an old file and reading a value the current binary wrote are different
facts, and only the second one is guaranteed to equal `FORMAT_VERSION`.

## Consumed token

A value that one step produces and exactly one later step requires is
a token, so the later step cannot run without the earlier one.
`VerdictCache` pairs a lookup with a store through `InputMissToken`
and `MissToken`.

Not for a retryable step. `#[must_use]` is a lint, not a guarantee:
`let _ = token` still discards it.

## Closed vocabulary

A fieldless enum with a fixed label set is declared through
`vocabulary!`, so the `serde` spelling, `Display` and `FromStr` come
from one list of literals instead of three that drift apart.
`ProjectKind` once spelled `self` two ways for this reason.

Not for a variant with a payload, or an enum whose wire spelling
differs from its display spelling on purpose (`IndirectCallForm`).

`vocabulary!` and `string_newtype!` are the two macros this tree
admits, both for the same reason: a table of literals has no
type-level expression.

## Declined generalizations

* a `&'static str` twin of each `string_newtype!` kind so placeholder
  constants can be `const`: five sites are not worth sixteen types
* a `BoundedString<const MAX: usize>` behind the name kinds: they
  disagree on charset, not only on length (`TagName` admits `.` and
  `+`, `ProjectName` does not)
* a generic `Versioned<T, const V: u32>` for the envelope, the snapshot
  header and the cache entry: an unknown version means refuse,
  migrate and skip respectively, three rules for three types
* `ProjectLayout` arms carrying their own `app_roots`: `erlang_otp`'s
  defaults fill the list after the emptiness check, so the rule would
  move into the defaults merge, which is the harder place to read it
* an `Unanalyzed` enum replacing `PatchedFile.binary: bool`: a binary
  file is a file with no text, and every reader asks only that
