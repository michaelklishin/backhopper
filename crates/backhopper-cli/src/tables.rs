//! Renderers for text-mode output: every table takes a `bel7_cli::TableStyle`
//! so the global `--table-style` flag governs all tables uniformly.

use bel7_cli::TableStyle;
use tabled::{Table, Tabled};

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolKind;
use backhopper_core::model::verdict::{PinVerdict, Reason, SeriesEvaluation, Verdict};

#[derive(Tabled)]
struct ReasonRow {
    pin: String,
    verdict: &'static str,
    reason: &'static str,
    detail: String,
}

pub fn render_evaluation_table(evaluation: &SeriesEvaluation, style: TableStyle) -> String {
    let mut t = Table::new(collect_rows(&evaluation.verdict.results));
    style.apply(&mut t);
    t.to_string()
}

fn collect_rows(results: &[PinVerdict]) -> Vec<ReasonRow> {
    let mut rows: Vec<ReasonRow> = Vec::new();
    for r in results {
        let pin = format_pin(&r.pin);
        let verdict_label = verdict_label(&r.verdict);
        if r.verdict.reasons().is_empty() {
            let symbols = r.tracked_refs;
            rows.push(ReasonRow {
                pin,
                verdict: verdict_label,
                reason: "-",
                detail: format!(
                    "{symbols} tracked symbol{} referenced",
                    if symbols == 1 { "" } else { "s" }
                ),
            });
            continue;
        }
        for reason in r.verdict.reasons() {
            rows.push(ReasonRow {
                pin: pin.clone(),
                verdict: verdict_label,
                reason: reason_kind(reason),
                detail: reason_detail(reason),
            });
        }
    }
    rows
}

fn verdict_label(v: &Verdict) -> &'static str {
    match v {
        Verdict::Compatible => "Compatible",
        Verdict::RequiresAdaptation { .. } => "RequiresAdaptation",
        Verdict::Incompatible { .. } => "Incompatible",
    }
}

fn format_pin(pin: &Pin) -> String {
    format!("{}@{}", pin.project, pin.tag)
}

fn reason_kind(r: &Reason) -> &'static str {
    match r {
        Reason::MissingSymbol { .. } => "MissingSymbol",
        Reason::ArityChanged { .. } => "ArityChanged",
        Reason::SignatureChanged { .. } => "SignatureChanged",
        Reason::FileAbsent { .. } => "FileAbsent",
        Reason::ContextDrift { .. } => "ContextDrift",
        Reason::DeprecatedUsage { .. } => "Deprecated",
        Reason::NowHidden { .. } => "NowHidden",
        Reason::RecordFieldsChanged { .. } => "RecordFields",
        Reason::UnsupportedFileType { .. } => "UnsupportedFileType",
        Reason::UntrackedModuleMissing { .. } => "UntrackedModuleMissing",
        Reason::ClauseMismatch { .. } => "ClauseMismatch",
    }
}

fn reason_detail(r: &Reason) -> String {
    match r {
        Reason::MissingSymbol { symbol, .. } => format_symbol(&symbol.kind),
        Reason::ArityChanged {
            module,
            function,
            expected,
            found,
        } => {
            let found_str = found
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{module}:{function} expected /{expected}, snapshot has /{found_str}")
        }
        Reason::SignatureChanged {
            module,
            function,
            arity,
            expected_spec,
            found_spec,
        } => format!(
            "{module}:{function}/{arity}: expected {expected_spec}; snapshot has {found_spec}"
        ),
        Reason::FileAbsent { path } => path.display().to_string(),
        Reason::ContextDrift { path, hunk_index } => {
            format!("{} (hunk #{hunk_index})", path.display())
        }
        Reason::DeprecatedUsage {
            symbol,
            since,
            replacement,
        } => {
            let s = format_symbol(&symbol.kind);
            match (since, replacement) {
                (Some(tag), Some(rep)) => {
                    format!("{s} (since {tag}; use {})", format_symbol(&rep.kind))
                }
                (Some(tag), None) => format!("{s} (since {tag})"),
                (None, Some(rep)) => format!("{s} (use {})", format_symbol(&rep.kind)),
                (None, None) => s,
            }
        }
        Reason::NowHidden { module } => format!("{module} is hidden at this pin"),
        Reason::RecordFieldsChanged {
            record,
            expected,
            found,
        } => {
            let e = expected
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let g = found
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("#{record}: expected fields [{e}]; snapshot has [{g}]")
        }
        Reason::UnsupportedFileType { path } => path.display().to_string(),
        Reason::UntrackedModuleMissing { module } => {
            format!("{module}.erl is absent in the target checkout")
        }
        Reason::ClauseMismatch {
            module,
            function,
            arity,
            call_args,
            pin_clauses,
        } => {
            let call = format_arg_shapes(call_args);
            let pins = pin_clauses
                .iter()
                .map(|c| format_arg_shapes(c))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("{module}:{function}/{arity}: called with ({call}); pin clause heads: ({pins})")
        }
    }
}

fn format_arg_shapes(args: &[ArgShape]) -> String {
    args.iter()
        .map(format_arg_shape)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_arg_shape(a: &ArgShape) -> String {
    match a {
        ArgShape::Variable => "_".to_string(),
        ArgShape::Atom { name } => format!("'{name}'"),
        ArgShape::Integer => "int".to_string(),
        ArgShape::Float => "float".to_string(),
        ArgShape::Binary => "<<>>".to_string(),
        ArgShape::List => "[..]".to_string(),
        ArgShape::Tuple { size } => format!("{{{size}-tuple}}"),
        ArgShape::Record { name } => format!("#{name}{{}}"),
        ArgShape::String => "\"\"".to_string(),
        ArgShape::Fun => "fun".to_string(),
        ArgShape::Unknown => "?".to_string(),
    }
}

fn format_symbol(kind: &SymbolKind) -> String {
    match kind {
        SymbolKind::Function { mfa } => mfa.to_string(),
        SymbolKind::Type {
            module,
            name,
            arity,
        } => format!("{module}:{name}/{arity}"),
        SymbolKind::Record { name } => format!("#{name}"),
        SymbolKind::Macro { name } => format!("?{name}"),
        SymbolKind::Behaviour { module } => format!("behaviour {module}"),
        SymbolKind::Callback {
            module,
            function,
            arity,
        } => format!("callback {module}:{function}/{arity}"),
    }
}
