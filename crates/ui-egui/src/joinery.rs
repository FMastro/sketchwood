//! The Joinery panel, in the tray when two parts (groups or components) are selected: the
//! joints their position allows, and in Entity Info, the joints a part has.

use eframe::egui;
use woodsketch_doc::joinery::{Fit, Tenon};
use woodsketch_doc::units::{format_length, parse_length};
use woodsketch_doc::{Document, Entity, InstanceId};

/// The two selected parts, if exactly two groups or components are selected.
pub fn selected_pair(doc: &Document) -> Option<(InstanceId, InstanceId)> {
    let parts: Vec<InstanceId> = doc.selection().iter().filter_map(|e| if let Entity::Instance(i) = e { Some(i) } else { None }).collect();
    match parts[..] {
        [a, b] if doc.selection().len() == 2 => Some((a, b)),
        _ => None,
    }
}

pub fn show(ui: &mut egui::Ui, doc: &mut Document, (a, b): (InstanceId, InstanceId)) {
    let name = |doc: &Document, id: InstanceId| doc.instance_name(id).unwrap_or("Part").to_owned();
    let (an, bn) = (name(doc, a), name(doc, b));
    let message_id = egui::Id::new("joinery_message");
    let mut message: Option<String> = None;
    match doc.joint_fit(a, b) {
        Err(e) => {
            ui.weak(e.to_string());
        }
        Ok(Fit::Apart) => {
            ui.weak(
                "These parts don't meet yet. For a dado, groove or rabbet, move one into the other by the \
                 joint's depth. For a mortise and tenon, butt a rail's end against the other part.",
            );
        }
        Ok(Fit::Overlap { half_lap }) => {
            ui.label("They overlap. Cut one's shape out of the other (a dado, groove, rabbet or notch):");
            for (cut, into, cn, inn) in [(a, b, &an, &bn), (b, a, &bn, &an)] {
                if ui.button(format!("Cut {cn} into {inn}")).on_hover_text(format!("{cn} keeps its size; {inn} gets the cut")).clicked() {
                    message = Some(match doc.cut_overlap(cut, into) {
                        Ok(kind) => format!("{} cut in {inn}.", kind.name()),
                        Err(e) => e.to_string(),
                    });
                }
            }
            if half_lap {
                ui.separator();
                ui.label("They cross through each other's thickness:");
                if ui.button("Half-lap").on_hover_text(format!("{an} loses its upper half of the overlap, {bn} its lower half")).clicked() {
                    message = Some(match doc.half_lap(a, b) {
                        Ok(()) => "Half-lap made.".into(),
                        Err(e) => e.to_string(),
                    });
                }
            }
        }
        Ok(Fit::EndToFace { rail, tenon }) => {
            let stile = if rail == a { b } else { a };
            let (rn, sn) = (name(doc, rail), name(doc, stile));
            ui.label(format!("{rn}'s end meets {sn}: a mortise and tenon."));
            // The sizes, editable, kept while the same pair is selected.
            let fields_id = egui::Id::new(("tenon_fields", rail, stile));
            let mut fields = ui
                .data(|d| d.get_temp::<[String; 3]>(fields_id))
                .unwrap_or_else(|| [tenon.length, tenon.thickness, tenon.shoulder].map(format_length));
            egui::Grid::new("tenon").num_columns(2).show(ui, |ui| {
                for (label, field, hint) in [
                    ("Length", 0, "How far the tenon goes into the mortise"),
                    ("Thickness", 1, "About a third of the rail is usual"),
                    ("Shoulders", 2, "Set in from the rail's edges, each side"),
                ] {
                    ui.label(label).on_hover_text(hint);
                    ui.add(egui::TextEdit::singleline(&mut fields[field]).desired_width(70.0));
                    ui.end_row();
                }
            });
            ui.data_mut(|d| d.insert_temp(fields_id, fields.clone()));
            let parsed = fields.iter().map(|f| parse_length(f).ok()).collect::<Vec<_>>();
            let sizes = match parsed[..] {
                [Some(length), Some(thickness), Some(shoulder)] => Some(Tenon { length, thickness, shoulder }),
                _ => None,
            };
            if ui.add_enabled(sizes.is_some(), egui::Button::new(format!("Tenon on {rn}, mortise in {sn}"))).clicked()
                && let Some(sizes) = sizes
            {
                message = Some(match doc.mortise_and_tenon(rail, stile, sizes) {
                    Ok(()) => format!("Mortise and tenon made; {rn} is now {} longer in the cut list.", format_length(sizes.length)),
                    Err(e) => e.to_string(),
                });
            }
        }
    }
    if let Some(m) = message {
        ui.data_mut(|d| d.insert_temp(message_id, (a, b, m)));
    }
    if let Some((ma, mb, m)) = ui.data(|d| d.get_temp::<(InstanceId, InstanceId, String)>(message_id))
        && (ma, mb) == (a, b)
    {
        ui.weak(m);
    }
}

/// The joints a part has, for Entity Info.
pub fn part_joints(ui: &mut egui::Ui, doc: &Document, part: InstanceId) {
    let joints = doc.joints_of(part);
    if joints.is_empty() {
        return;
    }
    ui.label("Joints");
    for (joint, other) in joints {
        ui.weak(format!("  {} with {other}, {}", joint.kind.name(), joint.detail));
    }
}
