//! `tabled` renderers used by text-mode output.

use tabled::{Table, Tabled, settings::Style};

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

pub fn render_evaluation_table(evaluation: &SeriesEvaluation) -> String {
    Table::new(collect_rows(&evaluation.verdict.results))
        .with(Style::modern())
        .to_string()
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
