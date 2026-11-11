//! Focused dialogs: the custom resolver editor, apply/revert confirmation, and the Android
//! setup sheet.
//!
//! Only one dialog is shown at a time, and Escape closes the topmost one. Everything here emits
//! `UiAction`s; the app owns validation, persistence and platform calls.

use eframe::egui::{self, Frame, Id, Margin, RichText, Stroke};

use crate::android_guide::GuideMode;
use crate::gui::actions::{RunScope, UiAction};
use crate::gui::components::{self, ButtonKind, Tone};
use crate::gui::icons::{self, Icon};
use crate::gui::state::{AppState, LayoutMode, ViewContext};
use crate::gui::theme::{self, Tokens};
use crate::servers::Protocol;

const DIALOG_WIDTH: f32 = 470.0;
const FIELD_HEIGHT: f32 = 40.0;

/// Show the topmost dialog, if any. Escape closes it before it can be drawn.
pub fn render(
    state: &mut AppState,
    cx: &ViewContext,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
) {
    let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    if escape {
        if state.custom_open {
            actions.push(UiAction::CancelCustom);
            return;
        }
        if state.guide.is_some() {
            actions.push(UiAction::CloseGuide);
            return;
        }
        if state.confirm_apply {
            actions.push(UiAction::CancelApply);
            return;
        }
        if state.confirm_revert {
            actions.push(UiAction::CancelRevert);
            return;
        }
        if cx.layout == LayoutMode::Mobile && state.details_open {
            actions.push(UiAction::CloseDetails);
            return;
        }
    }

    let tokens = theme::tokens(state.settings.theme, ctx.theme());
    let touch = cx.layout == LayoutMode::Mobile;
    if state.custom_open {
        custom_editor(state, ctx, actions, tokens, touch);
    } else if state.guide.is_some() {
        guide_sheet(state, ctx, actions, tokens, touch);
    } else if state.confirm_apply {
        apply_confirm(state, ctx, actions, tokens, touch);
    } else if state.confirm_revert {
        revert_confirm(state, ctx, actions, tokens, touch);
    } else if cx.layout == LayoutMode::Mobile && state.details_open {
        row_detail_sheet(state, cx, ctx, actions);
    }
}

fn dialog(id: &str, tokens: Tokens) -> egui::Modal {
    egui::Modal::new(Id::unique(id)).frame(
        Frame::new()
            .fill(tokens.surface)
            .stroke(Stroke::new(1.0, tokens.border))
            .corner_radius(14)
            .inner_margin(Margin::same(20)),
    )
}

/// One adaptive dialog body: the sheet never grows wider than the phone and scrolls when the
/// keyboard or a landscape window leaves less height than the content needs.
fn dialog_body(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut egui::Ui),
) {
    let width = dialog_width(ctx);
    ui.set_width(width);
    let max_height = (ctx.content_rect().height() - 96.0).max(120.0);
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_width(width)
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn field_height(touch: bool) -> f32 {
    if touch { 48.0 } else { FIELD_HEIGHT }
}

fn text_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    tokens: Tokens,
    touch: bool,
) {
    let label = ui.label(RichText::new(label).size(11.0).color(tokens.muted));
    ui.add_sized(
        [ui.available_width(), field_height(touch)],
        egui::TextEdit::singleline(value).hint_text(hint),
    )
    .labelled_by(label.id);
}

fn custom_editor(
    state: &mut AppState,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    let editing = state.custom_edit.is_some();
    let draft = &mut state.custom_draft;
    dialog("custom-resolver-dialog", tokens).show(ctx, |ui| {
        dialog_body(ctx, ui, "custom-resolver-scroll", |ui| {
            ui.label(
                RichText::new(if editing {
                    "Edit custom resolver"
                } else {
                    "Add custom resolver"
                })
                .size(20.0)
                .strong()
                .color(tokens.text),
            );
            ui.label(
                RichText::new(
                    "Custom resolvers are tested exactly like built-in servers. Plain, DoT and DoH \
                 endpoints are supported.",
                )
                .size(12.5)
                .color(tokens.muted),
            );
            ui.add_space(10.0);

            text_field(ui, "Name", &mut draft.name, "Optional label", tokens, touch);
            ui.add_space(8.0);

            field_label(ui, "Protocol", tokens);
            let options = Protocol::all().map(|protocol| (protocol, protocol.label()));
            let _ =
                components::segmented(ui, "custom-protocol", &mut draft.protocol, &options, touch);
            ui.add_space(8.0);

            match draft.protocol {
                Protocol::Udp | Protocol::Tcp => {
                    text_field(
                        ui,
                        "Address",
                        &mut draft.endpoint,
                        "1.1.1.1 or 1.1.1.1:5353",
                        tokens,
                        touch,
                    );
                    ui.label(
                    RichText::new(
                        "Custom ports are useful for testing; the Apply button only accepts port \
                         53.",
                    )
                    .size(11.5)
                    .color(tokens.muted),
                );
                }
                Protocol::Dot => {
                    text_field(
                        ui,
                        "Hostname",
                        &mut draft.endpoint,
                        "dns.example.com",
                        tokens,
                        touch,
                    );
                    ui.add_space(8.0);
                    text_field(
                        ui,
                        "IP address (optional)",
                        &mut draft.address,
                        "1.2.3.4",
                        tokens,
                        touch,
                    );
                }
                Protocol::Doh => {
                    text_field(
                        ui,
                        "DoH URL",
                        &mut draft.endpoint,
                        "https://dns.example.com/dns-query",
                        tokens,
                        touch,
                    );
                }
            }

            if let Some(error) = &draft.error {
                ui.add_space(8.0);
                components::pill(ui, error, Tone::Danger);
            }

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                if components::button(ui, "Cancel custom resolver", ButtonKind::Ghost, touch, true)
                    .clicked()
                {
                    actions.push(UiAction::CancelCustom);
                }
                if components::button(
                    ui,
                    if editing {
                        "Save resolver"
                    } else {
                        "Add resolver"
                    },
                    ButtonKind::Primary,
                    touch,
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::SaveCustom);
                }
            });
        });
    });
}

fn guide_sheet(
    state: &AppState,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    let Some(guide) = &state.guide else {
        return;
    };

    let (title, value) = match guide.mode {
        GuideMode::PrivateDns => (
            "Set Private DNS",
            guide.hostname.clone().unwrap_or_default(),
        ),
        GuideMode::WifiDns => (
            "Set Wi-Fi DNS",
            guide
                .plain_ips
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        GuideMode::NoHostname => ("No value to copy yet", String::new()),
    };

    dialog("android-guide-dialog", tokens).show(ctx, |ui| {
        dialog_body(ctx, ui, "android-guide-scroll", |ui| {
            ui.horizontal(|ui| {
                icons::paint_in(ui, Icon::Info, 20.0, tokens.accent, egui::Sense::hover());
                ui.label(RichText::new(title).size(20.0).strong().color(tokens.text));
            });
            ui.add_space(6.0);
            ui.label(RichText::new(&guide.note).size(12.5).color(tokens.muted));

            if !value.is_empty() {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(if guide.mode == GuideMode::PrivateDns {
                        "Hostname"
                    } else {
                        "DNS addresses"
                    })
                    .size(11.0)
                    .color(tokens.muted),
                );
                ui.label(
                    RichText::new(&value)
                        .monospace()
                        .size(15.0)
                        .strong()
                        .color(tokens.text),
                );
                if guide.mode == GuideMode::PrivateDns {
                    ui.label(
                        RichText::new(if guide.hostname_is_measured {
                            "Measured with DNS-over-TLS in this run."
                        } else {
                            "From provider metadata. Run DNS-over-TLS to measure it directly."
                        })
                        .size(11.5)
                        .color(tokens.muted),
                    );
                }
            }

            ui.add_space(10.0);
            for (index, step) in guide.steps.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.label(
                        RichText::new(format!("{}.", index + 1))
                            .monospace()
                            .size(12.0)
                            .color(tokens.accent),
                    );
                    // A horizontal layout hands its children the full row, so the step text
                    // must be explicitly wrapped or a long instruction widens the whole sheet.
                    ui.add(
                        egui::Label::new(RichText::new(step).size(12.5).color(tokens.text)).wrap(),
                    );
                });
            }

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                if !value.is_empty()
                    && components::button(ui, "Copy setup value", ButtonKind::Primary, touch, true)
                        .clicked()
                {
                    actions.push(UiAction::CopyGuideText);
                }
                match guide.mode {
                    GuideMode::PrivateDns => {
                        if components::button(
                            ui,
                            "Open Private DNS",
                            ButtonKind::Secondary,
                            touch,
                            true,
                        )
                        .clicked()
                        {
                            actions.push(UiAction::OpenPrivateDns);
                        }
                    }
                    GuideMode::WifiDns => {
                        if components::button(ui, "Open Wi-Fi", ButtonKind::Secondary, touch, true)
                            .clicked()
                        {
                            actions.push(UiAction::OpenWifi);
                        }
                    }
                    GuideMode::NoHostname => {}
                }
                if components::button(ui, "Close guide", ButtonKind::Ghost, touch, true).clicked() {
                    actions.push(UiAction::CloseGuide);
                }
            });

            // A metadata-only hostname has not been measured over DoT, and a missing one cannot be
            // recommended at all. Offer the run that produces a directly measured value.
            let offer_dot = guide.needs_dot_run
                || (guide.mode == GuideMode::PrivateDns && !guide.hostname_is_measured);
            if offer_dot {
                ui.add_space(4.0);
                if components::button(
                    ui,
                    "Benchmark DoT servers instead",
                    ButtonKind::Secondary,
                    touch,
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::CloseGuide);
                    actions.push(UiAction::SetProtocol(Protocol::Dot));
                    actions.push(UiAction::Run(RunScope::All));
                }
            }
            if guide.needs_plain_run {
                ui.add_space(4.0);
                if components::button(
                    ui,
                    "Benchmark plain DNS instead",
                    ButtonKind::Secondary,
                    touch,
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::CloseGuide);
                    actions.push(UiAction::SetProtocol(Protocol::Udp));
                    actions.push(UiAction::Run(RunScope::All));
                }
            }
        });
    });
}

fn apply_confirm(
    state: &AppState,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    let Some(pending) = &state.pending_apply else {
        return;
    };

    dialog("apply-confirm-dialog", tokens).show(ctx, |ui| {
        dialog_body(ctx, ui, "apply-confirm-scroll", |ui| {
            ui.label(
                RichText::new("Change system DNS?")
                    .size(20.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("dnsrace will use {}", pending.label))
                    .size(13.0)
                    .color(tokens.text),
            );
            ui.label(
                RichText::new(
                    pending
                        .addresses
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                )
                .monospace()
                .size(13.0)
                .color(tokens.muted),
            );

            if let Some(warning) = &pending.warning {
                ui.add_space(8.0);
                components::pill(ui, warning, Tone::Warning);
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Your system may ask for administrator permission. The previous settings are \
                 saved so you can restore them here.",
                )
                .size(12.0)
                .color(tokens.muted),
            );

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                if components::button(ui, "Cancel apply", ButtonKind::Ghost, touch, true).clicked()
                {
                    actions.push(UiAction::CancelApply);
                }
                if components::button(ui, "Apply DNS", ButtonKind::Primary, touch, true).clicked() {
                    actions.push(UiAction::ConfirmApply);
                }
            });
        });
    });
}

fn revert_confirm(
    state: &AppState,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
    tokens: Tokens,
    touch: bool,
) {
    dialog("revert-confirm-dialog", tokens).show(ctx, |ui| {
        dialog_body(ctx, ui, "revert-confirm-scroll", |ui| {
            ui.label(
                RichText::new("Restore previous DNS?")
                    .size(20.0)
                    .strong()
                    .color(tokens.text),
            );
            ui.add_space(4.0);
            if let Some(applied) = &state.applied {
                ui.label(
                    RichText::new("dnsrace will restore the settings saved when it applied:")
                        .size(12.5)
                        .color(tokens.text),
                );
                ui.label(
                    RichText::new(applied.summary())
                        .monospace()
                        .size(12.0)
                        .color(tokens.muted),
                );
            } else {
                ui.label(
                    RichText::new("No saved DNS change was found.")
                        .size(12.5)
                        .color(tokens.muted),
                );
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Your system may ask for administrator permission. If the saved settings are \
                 unavailable, the connection's automatic DNS is restored.",
                )
                .size(12.0)
                .color(tokens.muted),
            );

            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                if components::button(ui, "Cancel revert", ButtonKind::Ghost, touch, true).clicked()
                {
                    actions.push(UiAction::CancelRevert);
                }
                if components::button(ui, "Restore DNS", ButtonKind::Primary, touch, true).clicked()
                {
                    actions.push(UiAction::ConfirmRevert);
                }
            });
        });
    });
}

fn field_label(ui: &mut egui::Ui, text: &str, tokens: Tokens) {
    ui.label(RichText::new(text).size(11.0).color(tokens.muted));
}

/// A compact row-detail sheet used by the mobile shell. Desktop keeps its persistent detail
/// pane on the Servers page.
pub fn row_detail_sheet(
    state: &AppState,
    cx: &ViewContext,
    ctx: &egui::Context,
    actions: &mut Vec<UiAction>,
) {
    let Some(id) = state.selected_row.as_deref() else {
        return;
    };
    let measured = state.find_row(id);
    let candidate = state.find_candidate(id);
    if measured.is_none() && candidate.is_none() {
        return;
    }
    let label = measured
        .map(|row| row.label.clone())
        .or_else(|| {
            candidate.map(|candidate| {
                if candidate.label.is_empty() {
                    candidate.provider.clone()
                } else {
                    candidate.label.clone()
                }
            })
        })
        .unwrap_or_default();
    let endpoint = measured
        .map(|row| row.endpoint.clone())
        .or_else(|| candidate.map(crate::servers::Candidate::endpoint))
        .unwrap_or_default();
    let description = measured
        .map(|row| row.description.clone())
        .or_else(|| candidate.map(|candidate| candidate.note.clone()))
        .unwrap_or_default();
    let website = state.website_for(id);
    let favourite = state.settings.favorites.contains(id);
    let tokens = theme::tokens(state.settings.theme, ctx.theme());

    dialog("row-detail-dialog", tokens).show(ctx, |ui| {
        dialog_body(ctx, ui, ("row-detail-scroll", state.details_epoch), |ui| {
            // Give the close button its width first, then truncate the title into what is
            // left. A plain horizontal row let a long resolver name paint under the button
            // on a 360 dp phone.
            egui::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&label).size(20.0).strong().color(tokens.text),
                        )
                        .truncate(),
                    );
                },
                |ui| {
                    if components::button(ui, "Close details", ButtonKind::Ghost, true, true)
                        .clicked()
                    {
                        actions.push(UiAction::CloseDetails);
                    }
                },
            );
            ui.label(
                RichText::new(&endpoint)
                    .monospace()
                    .size(12.0)
                    .color(tokens.muted),
            );
            if !description.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(&description).size(12.5).color(tokens.text));
            }
            ui.add_space(8.0);
            if let Some(row) = measured {
                ui.horizontal_wrapped(|ui| {
                    components::metric(
                        ui,
                        "Cached",
                        &format!("{:.1} ms", row.cached.unwrap_or(row.score)),
                        None,
                    );
                    components::metric(
                        ui,
                        "Uncached",
                        &row.uncached
                            .map(|value| format!("{value:.1} ms"))
                            .unwrap_or_else(|| "Not measured".to_string()),
                        None,
                    );
                    components::metric(
                        ui,
                        "Jitter",
                        &format!("{:.1} ms", row.cached_jitter.unwrap_or(0.0)),
                        None,
                    );
                });
            } else {
                components::pill(ui, "Not measured yet", Tone::Neutral);
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if components::button(ui, "Choose this resolver", ButtonKind::Primary, true, true)
                    .clicked()
                {
                    actions.push(UiAction::ChooseRow(id.to_string()));
                }
                if components::button(
                    ui,
                    if favourite {
                        "Remove from favourites"
                    } else {
                        "Add to favourites"
                    },
                    ButtonKind::Secondary,
                    true,
                    true,
                )
                .clicked()
                {
                    actions.push(UiAction::ToggleFavorite(id.to_string()));
                }
                if components::button(ui, "Copy endpoint", ButtonKind::Secondary, true, true)
                    .clicked()
                {
                    actions.push(UiAction::CopyEndpoint(id.to_string()));
                }
                if website.is_some()
                    && components::button(ui, "Open website", ButtonKind::Secondary, true, true)
                        .clicked()
                {
                    actions.push(UiAction::OpenWebsite(id.to_string()));
                }
                if cx.capabilities.is_android()
                    && components::button(
                        ui,
                        "Set up this resolver",
                        ButtonKind::Primary,
                        true,
                        true,
                    )
                    .clicked()
                {
                    actions.push(UiAction::OpenGuide(id.to_string()));
                }
                if cx.capabilities.can_apply_dns
                    && components::button(
                        ui,
                        "Use this resolver",
                        ButtonKind::Secondary,
                        true,
                        measured.is_some(),
                    )
                    .clicked()
                {
                    actions.push(UiAction::Apply(id.to_string()));
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new("Details close with Escape or the close button.")
                    .size(11.0)
                    .color(tokens.muted),
            );
        });
    });
}

/// Adaptive content width for small screens. The frame adds 20 pt of inner margin on each side
/// and the caller keeps 16 pt of screen edge, so 72 pt is reserved before clamping.
fn dialog_width(ctx: &egui::Context) -> f32 {
    (ctx.content_rect().width() - 72.0).clamp(240.0, DIALOG_WIDTH)
}
