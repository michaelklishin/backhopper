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
`ProjectName`, `CommitSha`. The Cuttlefish and erlang.mk kinds follow
the same rule: `ConfKey` admits `$name` segments, `PartialName` is a
file stem and refuses `/` and `.`, `ValidatorName`, `MakeVariableName`.
`HostName` has a project name's charset, `ExtractorVersion` is a short
dotted token, and `SeriesTagPattern` is a tag glob with exactly one
`{series}` placeholder, whose `for_series` is its one method.

Not for a pattern or free text. `TagGlob` is its own type, not a
`TagName`, because a pattern is not a name.

The kinds implement `Borrow<str>`, so a set of names answers
`contains` for a scanned `&str`. None implements `Deref` or
`From<String>`: that is what stops a call site from erasing the type.
All of them are in `model/names.rs`.

## Non-empty collection

A list that must have a first element is `NonEmpty<T>`, whose
constructors (`NonEmpty::new(first, rest)` and `TryFrom<Vec<T>>`) are
its only doors. `first()` and `last()` return `&T`, not an `Option`,
and serde goes through `TryFrom`, so a wire value with an empty list is
refused where it is read. `sort_dedup` keeps it non-empty.

`HostReleaseFacts.release_roots` is one: a host snapshot with roots
and no apps to root them at is not a value. `ReleaseRoots` is the same
rule one layer out, an enum of `Apps`, `Variable` and `Both` rather
than two optional fields.

`SeriesReleases<R>` holds the patch releases of one series ascending, with
at least one. `new` is the only constructor and sorts, then refuses an
empty list, a duplicate and a second series, so `build()` (the newest) is
`NonEmpty::last` and never an index into a list a caller may not have
sorted. `map` builds through `new` again, because a mapped value carries
its own tag.

Not for a list that may legitimately be empty. No crate is taken for
it: the type is small, and the workspace avoids small dependencies.

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
digest), `HostFactsDigest` (sixty-four, the digest of the configuration
a host snapshot's roots and pins were read with), `StoreDigest` (sixty-four,
the digest of the snapshots a loaded plan read), `ExtractionSettingsDigest`
(sixty-four, the digest of the project settings that change what
extraction reads). The digest types have a `from_digest` that takes the
raw bytes and cannot fail.

`extraction_settings_digest` takes a `Project`, so the settings that go
into it are the fields of one struct and a setting added to extraction
has one place to be added. Each list setting is sorted and deduplicated
first, so the order of a list in the configuration cannot make a snapshot
look stale.

## Composite key

A type built from already-validated parts, with no rule across them,
can be built any way: every way produces a valid value. `Mfa` has
three validated parts and is built by `new` or by the struct literal.

`MacroKey` is a name and an optional arity: Erlang treats `NAME` and
`NAME(Args)` as different macros, so both parts are the identity, and
the derived order puts the object-like form first.

`PlanCacheKey::compose` builds `jigsaw-<host>-<series>-f<format>-e<extractor>-<digest>`
from typed parts, and `restore_prefix` is the part up to the digest, so
a workflow file never spells the format.

`UncheckedSymbol` has one variant per listed reason of `NotChecked`
(`UnresolvedAtBuild` over a `Reference`, `Dynamic` over a `DynamicCall`),
so `UncheckedSite::reason` is derived from the symbol and a site cannot be
listed under a reason that does not fit it. `Tallies::new` counts these
reasons from the list, so the tally and the list cannot differ.

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

A plan has the same shape one level up: `JigsawPlan` says what a run
needs, and `JigsawPlan::load` is the only way to a `LoadedPlan`, which
holds the snapshots it read from a store that had all of them in a
usable state. `evaluate` takes a `LoadedPlan` and nothing else, so a
run against missing or stale snapshots cannot be written, and the plan
and the snapshots it reads cannot come from two places. Both types keep
their fields private, and `JigsawPlan` has one private `assemble` that
`draft_plan` and `from_wire` share, so there is no plan with an
unsupported release and Erlang line pair, whichever way it was made.

A plugin set has the same shape on the way in: `PluginSources` is plain
data from the directory walk, `PluginSet::new` is the one door that
refuses a duplicate app or module name, and a `PluginSet` is the only
plugin input `evaluate` takes. Its `ReadManifest` is built only by
`AppManifest::into_read`, so nothing downstream matches on an
unreadable manifest. `evaluate` takes the function that reads the plugin
set against the build patch's surface as an argument, so a
`ReferenceSet` cannot be handed in from a different build patch.

Not for facts that can hold in any combination. Those are a struct of
flags, not a pipeline.

## Normalised text

Source text that two spellings of one thing must agree on is a newtype
whose constructor is the normaliser, so a value that exists is already
normalised and equal values are equal text. `NormalisedMacroBody` and
`NormalisedDefault` share one normaliser (comments stripped, whitespace
runs collapsed, raw line breaks inside literals written as escapes) and
stay two types, because a macro body and a record default do not
compare with each other.

Reading one back from a snapshot goes through `from_stored`, which
refuses text that normalising would change, so a hand-edited line is
refused as a non-canonical one is. An empty macro body is the token
`%empty%`, which no Erlang token sequence can be, so a snapshot line
cannot read as an absent body. `NormalisedDefault::from_source` returns
`None` for blank text: a field without a default is `None` on the
field, and a written `= undefined` is `Some`.

Whitespace that is absent is not inserted: `{a,b}` and `{a, b}` are two
values. The guarantee is about comments and about how much whitespace
sits where whitespace already was.

## Verdict without severity

`Finding` has no severity field and no `is_blocking`, because every
finding is a blocker. A class that is not one is a variant of `Warning`,
a separate type, and `JigsawReport::is_clean` reads only the findings, so
a warning cannot be mistaken for a blocker, or the reverse, through a
shared list. The report stores no matrix: `cell` answers from the
findings and their spans, so there is no second copy to disagree with
them. A finding is kept once with the targets it breaks on, and
`TargetAxes::runs` derives the runs from the member set, so "sorted,
disjoint and not adjacent" holds by construction.

## A reason that carries its subject

`WithheldSymbol` is a comparison the check did not make, and it carries
what the comparison was about: `ConditionalMacro { header, key }`,
`UnknownSpec(Mfa)`, `ExcludedOtpApp(ApplicationName)`. Its `reason()` is
derived from the variant, so a symbol cannot be listed under a reason that
does not fit it, and the tally of a listed reason is the number of sites
its entries hold. `Stop` is the other kind of withheld comparison, a
closure over manifests that could not go on, and has no subject because
the warnings name the apps. `AppPresence` (a host release) has `Stopped`
and `OtpAppPresence` has `Excluded`, because only one side can have each,
so neither enum has an arm the other side's lookups can never return.

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

The `jigsaw` builders put the argument itself in the marker. A
`JigsawPlanBuilder<H, S>` holds its host in `WithHost` and its series in
`WithSeries`. A `JigsawCheckBuilder<T, P>` holds its plan source in
`WithCheckTarget(CheckTarget)`, an identity choice between a plan file
and `PlanArguments`, and its plugin directories in
`WithPlugins(NonEmpty<PathBuf>)`: `plugin_dir` on `NoPlugins` creates the
list with its first element and on `WithPlugins` appends, so "at least one
directory" is held by the type and not by the builder's discipline.

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
`LineContextSource::{PostImage(text), HunksOnly}` for what the
attribute-region classifier reads, and the CLI's `PostImage::{AtCommit
{ repo, sha }, Unavailable}` for where that text comes from, so a
caller with a commit to read cannot pass nothing by accident and a
patch with no commit says so by name.

`PinSource::{ErlangMk { file_path }, Table(..)}` for where a host's
dependency pins come from, and `TagSourceKind::{Repository, Store}` for
where its release tags were listed, so a plan says which it used.
`ErlangSupport` is a default range plus a map of releases, and
`range_for` is its only lookup: the nearest entry at or below the
release, or the default, so the lookup cannot fail.

`FindingSpan::{Host(releases), Otp(tags), Everywhere}` for which targets
a finding covers: a host finding covers every OTP tag, an OTP finding
covers every host release, and an app nothing provides covers
everything, so a finding on some of both axes cannot be written down.
`SiteDetail::{Plain, Call(CallForm), Construction(RecordConstruction)}`
for what a reference site adds to its line, so a call form never sits on
a record site. `HostAnchor` and `OtpAnchor` for which side a reference
was anchored on, which decides whether it is checked once per host
release or once per OTP tag. `Lookup::{Found, Missing, Withheld}` for a
schema element in a release that may not ship the app that holds it.

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

`ManifestDeps` is `Listed` or `Unreadable(ManifestUnreadable)`, with
`Missing`, `UnexpandedVariable(name)` and `ParseFailed` as the grounds,
because a partial list would make an app look shippable when it is not.
`MacroBody` is `Text` or `Conditional`: a macro defined under a
conditional other than `TEST` or an include guard has no body to
compare, and says so instead of carrying a guess.
`Surface<T>::lookup` answers `Present`, `Absent` or
`Unknowable(Unreadable)` for an export surface, and has no `contains`
to mistake for a `bool`. `ExportedTypes` is `Listed` or `Unreadable`.
`IncludeCoverage` names which side of an include walk went unread, and
its two methods say what that can hide. `EvaluationFiles` is a
`BTreeMap<PathBuf, Option<Vec<u8>>>`: a missing key, `None` and bytes
are three readings.

`AppPresence` is `Provided`, `Absent` or `Withheld(WithheldReason)`.
`ShippedApps` answers it by `lookup` alone, because the closure of the
release roots over the manifests stops at an unreadable manifest or an
untracked dependency, and an app outside a closure that stopped short may
be one it did not reach. It does not reuse `Surface<T>::lookup`: that
type's third state is an export list behind a parse transform, which is
another ground.

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

`JigsawPlanPayload` and `JigsawCheckPayload` are wire structs of the first
kind's outer layer: flat fields, strings for versions, `Option` where a
value can be absent. `JigsawPlan` and `JigsawReport` are the types one
layer in, and each payload has one writer (`to_wire`, and
`JigsawCheckPayload::from(&JigsawReport)`), which is the only place `clean`
and `cells` are derived from `is_clean` and `cell`. The tally maps are keyed
by vocabularies, which `serde` writes as their labels.

The schema table is one row per version in ascending order, with a
`const` assertion on the length and on each row's position, so a
missing, duplicated or misplaced row is a compile error. The snapshot
format has `FORMAT_VERSION` and `SUPPORTED_FORMAT_VERSIONS`.
`SnapshotHeader.format_version` retains the value the parser actually
read, distinct from the constant a fresh generation sets: reading an
old file and reading a value the current binary generated are different
facts, and only the second one is guaranteed to equal `FORMAT_VERSION`.
The writer emits the snapshot's own version, never below
`LAST_MIGRATABLE_FORMAT_VERSION`, and refuses (`Unwritable`) a format 5
snapshot with a module or header that has no app, so a file the parser
would refuse is not one this binary writes. `Module.app` and
`HrlFile.app` stay `Option` because the one type reads every format.

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

`NotChecked`, `WithheldReason`, `JigsawFindingKind`, `JigsawWarningKind`,
`JigsawCell` and `AppRole` are vocabularies for a `jigsaw check` run.
`Finding::kind` and `Warning::kind` are exhaustive matches, so a label
cannot fall behind its enum. `Tallies` holds every label of the first two
with a zero where nothing was counted, so a consumer can tell zero from
not reported.

Not for a variant with a payload, or an enum whose wire spelling
differs from its display spelling on purpose (`IndirectCallForm`).

`vocabulary!` and `string_newtype!` are the two macros this tree
admits, both for the same reason: a table of literals has no
type-level expression.

## Declined generalizations

* one `Surface<T, G>` over export unreadability and app presence: the
  third states mean different things (a parse transform and a stopped
  closure), and a generic over both grounds is an abstraction over two
  meanings
* branding `HostIdx` and `OtpIdx` to one plan: `TargetAxes` is the only
  source of them and a report holds its own
* a `LineNumber` newtype for `ReferenceSite.line`: it is read in one
  place, to print it
* a `ReferenceSet::collect` method in `backhopper-core`: the call reader
  and the Cuttlefish parser depend on `backhopper-core`, so the reading
  is `backhopper_jigsaw::collect` and `ReferenceSet` is the data it fills

* a `TagPrefix` newtype for `Project.tag_prefix`: the plan parses its
  tags through the configured projects, so no plan type holds a prefix
* one `Digest<N>` type behind `HostFactsDigest`, `StoreDigest`, the
  plan digest and `VerdictFingerprint`: their lengths differ and so
  do their meanings, and they share only `lower_hex`

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
* one `Normalised<K>` type behind `NormalisedMacroBody` and
  `NormalisedDefault`: they share the normaliser function and nothing
  else, and a body is never compared with a default
* a `ManifestFile` trait over `.app.src` and erlang.mk manifests in
  `backhopper-core`: the CLI's two-arm enum is the only place that
  chooses between them
* one generic site list behind `unchecked` and `withheld`: `unchecked` is
  a row per site and a withheld entry is a symbol with its sites and the
  targets it was withheld at, so they agree on the site and not on the
  shape that holds it
* a non-empty set for `Anchor.sites`: `evaluate` turns each hit into a
  `NonEmpty` and drops one with no sites, which cannot happen because an
  anchor is built from at least one reference; the set would move that
  check into `Anchor`, which every anchor kind builds in its own place
* an `Unanalyzed` enum replacing `PatchedFile.binary: bool`: a binary
  file is a file with no text, and every reader asks only that
