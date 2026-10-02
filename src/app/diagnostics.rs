use super::*;
use crate::diagnostics::support::SupportReport;

pub(super) fn diagnostics_key(app: &mut App, code: KeyCode, tasks: &Tasks) {
    match code {
        KeyCode::Esc | KeyCode::F(7) | KeyCode::Char('q') => app.ui.close(Overlay::Diagnostics),
        KeyCode::Char('r') => {
            if app.ui.diagnostics.preview.take().is_some() {
                app.ui.diagnostics.preview_json.clear();
                app.ui.diagnostics.notice =
                    "Viewing recent errors; r creates a fresh report snapshot.";
            } else {
                let report = SupportReport::new(
                    &app.ui.diagnostics.history,
                    tasks.catalog.capability_summary(),
                    vec![],
                );
                match report.json() {
                    Ok(json) => {
                        app.ui.diagnostics.notice = "Report snapshot ready; scroll to inspect, then e exports exactly this snapshot.";
                        app.ui.diagnostics.preview_json = json;
                        app.ui.diagnostics.preview = Some(report);
                    }
                    Err(error) => {
                        app.ui.diagnostics.notice =
                            "Report preview failed; sensitive details were omitted.";
                        app.ui
                            .diagnostics
                            .history
                            .record(Subsystem::Diagnostics, &error);
                        app.status =
                            "Support preview failed; diagnostic details were omitted for privacy."
                                .into();
                    }
                }
            }
            app.ui.diagnostics.scroll = 0;
        }
        KeyCode::Char('e') => {
            if let Some(report) = &app.ui.diagnostics.preview {
                let name = SupportReport::suggested_name();
                let root = Storage::local_read_only().ok().map(|store| store.root);
                match report.export(std::path::Path::new(&name), root.as_deref()) {
                    Ok(_) => {
                        app.ui.diagnostics.notice =
                            "Reviewed report saved in the launch directory; nothing uploaded.";
                        app.ui.diagnostics.exported_name = Some(name.clone());
                        app.status = format!(
                            "Support report saved to {name} in the launch directory; inspect before sharing. Nothing uploaded."
                        );
                    }
                    Err(error) => {
                        app.ui.diagnostics.notice = "Export failed; check directory permissions and use a new file outside saved data.";
                        app.ui
                            .diagnostics
                            .history
                            .record(Subsystem::Diagnostics, &error);
                        app.status = "Support export failed. Choose a writable launch directory outside saved data; existing files are never overwritten.".into();
                    }
                }
            } else {
                app.ui.diagnostics.notice =
                    "Press r to inspect a report snapshot before exporting it.";
                app.status =
                    "Press r to inspect a redacted report snapshot before exporting it.".into();
            }
        }
        code => {
            let preview = app.ui.diagnostics.preview.is_some();
            let maximum = if preview {
                app.ui.render.borrow().diagnostics_length.saturating_sub(1)
            } else {
                app.ui.diagnostics.history.records().len().saturating_sub(1)
            };
            let selected = if preview {
                &mut app.ui.diagnostics.scroll
            } else {
                &mut app.ui.diagnostics.selected
            };
            *selected = match code {
                KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => selected.saturating_add(1).min(maximum),
                KeyCode::PageUp => selected.saturating_sub(10),
                KeyCode::PageDown => selected.saturating_add(10).min(maximum),
                KeyCode::Home => 0,
                KeyCode::End => maximum,
                _ => *selected,
            };
        }
    }
}
