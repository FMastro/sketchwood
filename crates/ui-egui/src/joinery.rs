//! The Joinery panel, in the tray when two parts (groups or components) are selected: pick a
//! joint, set its sizes, and make it. Joints that don't fit how the parts sit say how to place
//! them. Entity Info lists the joints a part has.

use eframe::egui;
use woodsketch_doc::joinery::{Dowels, Fit, Tenon};
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    LetIn,
    Overlap,
    MortiseAndTenon,
    Dowels,
    BoxJoint,
    HalfLap,
}

impl Choice {
    const ALL: [Self; 6] = [Self::LetIn, Self::MortiseAndTenon, Self::Dowels, Self::BoxJoint, Self::HalfLap, Self::Overlap];

    fn name(self) -> &'static str {
        match self {
            Self::LetIn => "Dado, groove or rabbet",
            Self::Overlap => "Cut where they overlap",
            Self::MortiseAndTenon => "Mortise and tenon",
            Self::Dowels => "Dowels",
            Self::BoxJoint => "Box joint",
            Self::HalfLap => "Half-lap",
        }
    }

    fn fits(self, fit: &Fit) -> bool {
        match self {
            Self::LetIn | Self::MortiseAndTenon | Self::Dowels => fit.butt.is_some(),
            Self::Overlap => fit.overlap,
            Self::BoxJoint => fit.corner,
            Self::HalfLap => fit.half_lap,
        }
    }

    /// How to place the parts for this joint.
    fn setup(self) -> &'static str {
        match self {
            Self::LetIn => "Butt one part's end or edge against the other (a shelf against a side, a panel against a post). It's let in by the depth: a dado across the grain, a groove along it, a rabbet at an edge.",
            Self::MortiseAndTenon => "Butt the rail's end against the other part (a rail against a leg or stile).",
            Self::Dowels => "Butt one part's end against the other.",
            Self::BoxJoint => "Put the two parts' ends together at a corner (a drawer's front and side), butted or overlapping.",
            Self::HalfLap => "Cross the two boards so they overlap through their whole thickness.",
            Self::Overlap => "Move one part into the other by the joint's depth (a shelf 1/4\" into the side).",
        }
    }
}

pub fn show(ui: &mut egui::Ui, doc: &mut Document, (a, b): (InstanceId, InstanceId)) {
    let name = |doc: &Document, id: InstanceId| doc.instance_name(id).unwrap_or("Part").to_owned();
    let fit = match doc.joint_fit(a, b) {
        Ok(fit) => fit,
        Err(e) => {
            ui.weak(e.to_string());
            return;
        }
    };
    let choice_id = egui::Id::new(("joint_choice", a, b));
    let suggested = Choice::ALL.into_iter().find(|c| c.fits(&fit)).unwrap_or(Choice::LetIn);
    let mut choice = ui.data(|d| d.get_temp::<Choice>(choice_id)).unwrap_or(suggested);
    ui.horizontal(|ui| {
        ui.label("Joint");
        egui::ComboBox::from_id_salt("joint_choice").selected_text(choice.name()).show_ui(ui, |ui| {
            for c in Choice::ALL {
                let text = if c.fits(&fit) { c.name().to_owned() } else { format!("{} (place the parts first)", c.name()) };
                ui.selectable_value(&mut choice, c, text);
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(choice_id, choice));

    let mut message: Option<String> = None;
    if !choice.fits(&fit) {
        ui.weak(choice.setup());
    } else {
        match choice {
            Choice::Overlap => {
                for (cut, into) in [(a, b), (b, a)] {
                    let (cn, inn) = (name(doc, cut), name(doc, into));
                    if ui.button(format!("Cut {cn} into {inn}")).on_hover_text(format!("{cn} keeps its size; {inn} gets the cut")).clicked() {
                        message = Some(result(doc.cut_overlap(cut, into).map(|k| format!("{} cut in {inn}.", k.name()))));
                    }
                }
            }
            Choice::HalfLap => {
                if ui.button("Make the half-lap").clicked() {
                    message = Some(result(doc.half_lap(a, b).map(|()| "Half-lap made.".to_owned())));
                }
            }
            Choice::BoxJoint => {
                let [finger] = fields(ui, ("box", a, b), [("Finger width", fit.finger, "Rounded so there's an odd number of fingers")]);
                if button(ui, finger.is_some(), "Make the box joint") {
                    let finger = finger.unwrap_or(fit.finger);
                    message = Some(result(doc.box_joint(a, b, finger).map(|()| "Box joint made.".to_owned())));
                }
            }
            Choice::LetIn | Choice::MortiseAndTenon | Choice::Dowels => {
                let (part, into) = fit.butt.expect("fits");
                let (pn, inn) = (name(doc, part), name(doc, into));
                ui.label(format!("{pn}'s end meets {inn}."));
                match choice {
                    Choice::LetIn => {
                        let [depth] = fields(ui, ("let_in", a, b), [("Depth", fit.let_in_depth, "How far it's let in; it grows by this")]);
                        if button(ui, depth.is_some(), &format!("Let {pn} into {inn}")) {
                            let depth = depth.unwrap_or(fit.let_in_depth);
                            message = Some(result(doc.let_in(part, into, depth).map(|k| {
                                format!("{} cut in {inn}; {pn} is {} longer.", k.name(), format_length(depth))
                            })));
                        }
                    }
                    Choice::MortiseAndTenon => {
                        let t = fit.tenon;
                        let [length, thickness, shoulder] = fields(
                            ui,
                            ("tenon", a, b),
                            [
                                ("Length", t.length, "How far the tenon goes into the mortise"),
                                ("Thickness", t.thickness, "About a third of the rail is usual"),
                                ("Shoulders", t.shoulder, "Set in from the rail's edges, each side"),
                            ],
                        );
                        if let (true, Some(length), Some(thickness), Some(shoulder)) =
                            (button(ui, length.is_some() && thickness.is_some() && shoulder.is_some(), &format!("Tenon on {pn}, mortise in {inn}")), length, thickness, shoulder)
                        {
                            message = Some(result(doc.mortise_and_tenon(part, into, Tenon { length, thickness, shoulder }).map(|()| {
                                format!("Mortise and tenon made; {pn} is {} longer in the cut list.", format_length(length))
                            })));
                        }
                    }
                    _ => {
                        let d = fit.dowels;
                        let [count, diameter, depth] = fields(
                            ui,
                            ("dowels", a, b),
                            [
                                ("Count", f64::from(d.count), "Spread evenly across the width"),
                                ("Diameter", d.diameter, "3/8\" for 3/4\" stock is usual"),
                                ("Depth", d.depth, "Hole depth in each part"),
                            ],
                        );
                        if let (true, Some(count), Some(diameter), Some(depth)) =
                            (button(ui, count.is_some() && diameter.is_some() && depth.is_some(), "Drill for dowels"), count, diameter, depth)
                        {
                            let dowels = Dowels { count: count.round().max(0.0) as u32, diameter, depth };
                            message = Some(result(doc.dowels(part, into, dowels).map(|()| format!("{} dowel holes drilled in each part.", dowels.count))));
                        }
                    }
                }
            }
        }
    }

    let message_id = egui::Id::new("joinery_message");
    if let Some(m) = message {
        ui.data_mut(|d| d.insert_temp(message_id, (a, b, m)));
    }
    if let Some((ma, mb, m)) = ui.data(|d| d.get_temp::<(InstanceId, InstanceId, String)>(message_id))
        && (ma, mb) == (a, b)
    {
        ui.weak(m);
    }
}

fn result(r: Result<String, woodsketch_doc::joinery::JoineryError>) -> String {
    r.unwrap_or_else(|e| e.to_string())
}

fn button(ui: &mut egui::Ui, enabled: bool, text: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(text)).clicked()
}

/// Editable sizes (kept while the same parts and joint are picked), parsed: `None` where the
/// text isn't a length.
fn fields<const N: usize>(ui: &mut egui::Ui, key: (&str, InstanceId, InstanceId), specs: [(&str, f64, &str); N]) -> [Option<f64>; N] {
    let id = egui::Id::new(key);
    let mut texts: Vec<String> = ui.data(|d| d.get_temp::<Vec<String>>(id)).unwrap_or_else(|| {
        specs.iter().map(|(label, value, _)| if *label == "Count" { format!("{value}") } else { format_length(*value) }).collect()
    });
    egui::Grid::new(id).num_columns(2).show(ui, |ui| {
        for (i, (label, _, hint)) in specs.iter().enumerate() {
            ui.label(*label).on_hover_text(*hint);
            ui.add(egui::TextEdit::singleline(&mut texts[i]).desired_width(70.0));
            ui.end_row();
        }
    });
    let parsed = std::array::from_fn(|i| parse_length(&texts[i]).ok().filter(|v| *v > 0.0));
    ui.data_mut(|d| d.insert_temp(id, texts));
    parsed
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
