//! The Joinery panel, in the tray when two parts (groups or components) are selected: pick a
//! joint, set its sizes, and make it. Joints that don't fit how the parts sit say how to place
//! them. Entity Info lists the joints a part has.

use eframe::egui;
use woodsketch_doc::joinery::{BISCUITS, Dovetails, Dowels, Fit, JointKind, LOOSE_TENONS, Slots, Spline, Tenon, pocket_screw_for};
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
    Biscuits,
    LooseTenons,
    PocketScrews,
    Dovetails,
    Splines,
    BoxJoint,
    HalfLap,
}

impl Choice {
    const ALL: [Self; 11] = [
        Self::LetIn,
        Self::MortiseAndTenon,
        Self::Dovetails,
        Self::Dowels,
        Self::Biscuits,
        Self::LooseTenons,
        Self::Splines,
        Self::PocketScrews,
        Self::BoxJoint,
        Self::HalfLap,
        Self::Overlap,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::LetIn => "Dado, groove or rabbet",
            Self::Overlap => "Cut where they overlap",
            Self::MortiseAndTenon => "Mortise and tenon",
            Self::Dowels => "Dowels",
            Self::Biscuits => "Biscuits",
            Self::LooseTenons => "Loose tenons (Domino style)",
            Self::PocketScrews => "Pocket screws",
            Self::Dovetails => "Dovetails",
            Self::Splines => "Spline",
            Self::BoxJoint => "Box joint",
            Self::HalfLap => "Half-lap",
        }
    }

    fn fits(self, fit: &Fit) -> bool {
        match self {
            Self::LetIn | Self::MortiseAndTenon | Self::Dowels | Self::Biscuits | Self::LooseTenons | Self::Splines | Self::PocketScrews => fit.butt.is_some(),
            Self::Dovetails => fit.butt.is_some() && fit.corner,
            Self::Overlap => fit.overlap,
            Self::BoxJoint => fit.corner,
            Self::HalfLap => fit.half_lap,
        }
    }

    /// The picker entry that makes a joint of this kind (for Change).
    fn of(kind: JointKind) -> Self {
        match kind {
            JointKind::MortiseAndTenon => Self::MortiseAndTenon,
            JointKind::Dowels => Self::Dowels,
            JointKind::Biscuits => Self::Biscuits,
            JointKind::LooseTenons => Self::LooseTenons,
            JointKind::PocketScrews => Self::PocketScrews,
            JointKind::Dovetails => Self::Dovetails,
            JointKind::Splines => Self::Splines,
            JointKind::BoxJoint => Self::BoxJoint,
            JointKind::HalfLap => Self::HalfLap,
            JointKind::Mortise | JointKind::Notch => Self::Overlap,
            JointKind::Dado | JointKind::Groove | JointKind::Rabbet => Self::LetIn,
        }
    }

    /// How to place the parts for this joint.
    fn setup(self) -> &'static str {
        match self {
            Self::LetIn => "Butt one part's end or edge against the other (a shelf against a side, a panel against a post). It's let in by the depth: a dado across the grain, a groove along it, a rabbet at an edge.",
            Self::MortiseAndTenon => "Butt the rail's end against the other part (a rail against a leg or stile).",
            Self::Dowels | Self::Biscuits | Self::LooseTenons | Self::Splines | Self::PocketScrews => "Butt one part's end or edge against the other.",
            Self::Dovetails => "Butt one board's end against the inside of the other at its end, like a drawer's side against its front. The butting board gets the tails.",
            Self::BoxJoint => "Put the two parts' ends together at a corner (a drawer's front and side), butted or overlapping.",
            Self::HalfLap => "Cross the two boards so they overlap through their whole thickness.",
            Self::Overlap => "Move one part into the other by the joint's depth (a shelf 1/4\" into the side).",
        }
    }
}

pub fn show(ui: &mut egui::Ui, doc: &mut Document, (a, b): (InstanceId, InstanceId)) {
    let fit = match doc.joint_fit(a, b) {
        Ok(fit) => fit,
        Err(e) => {
            ui.weak(e.to_string());
            return;
        }
    };
    // Already joined (here, or on other copies of the same components): say so, and keep the
    // picker for adding another joint out of the way.
    let other = doc.container().instance(b).map(|i| i.definition);
    let joints: Vec<_> = doc.joint_list(a).into_iter().filter(|(_, j, _)| Some(j.cut) == other || Some(j.into) == other).collect();
    if joints.is_empty() {
        picker(ui, doc, (a, b), &fit);
    } else {
        ui.label("Already joined:");
        let mut change = None;
        let mut remove = None;
        for (index, joint, _) in &joints {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} {}, {}", egui_phosphor::regular::CHECK, joint.kind.name(), joint.detail));
                if ui.small_button("Change").on_hover_text("Take it out and pick its sizes again").clicked() {
                    change = Some((*index, joint.kind));
                }
                if ui.small_button("Remove").on_hover_text("Take this joint out of both parts").clicked() {
                    remove = Some(*index);
                }
            });
        }
        let another = egui::Id::new(("another_joint", a, b));
        if let Some(index) = remove {
            let message = doc.remove_joint(index).map_or_else(|e| e.to_string(), |()| "Joint removed.".to_owned());
            ui.data_mut(|d| d.insert_temp(egui::Id::new("joinery_message"), (a, b, message)));
        }
        if let Some((index, kind)) = change {
            match doc.remove_joint(index) {
                Ok(()) => {
                    // Back to the picker on the same joint, to set it up again.
                    ui.data_mut(|d| {
                        d.insert_temp(egui::Id::new(("joint_choice", a, b)), Choice::of(kind));
                        d.insert_temp(another, true);
                        d.insert_temp(egui::Id::new("joinery_message"), (a, b, "Set it up again, then make it.".to_owned()));
                    });
                }
                Err(e) => {
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("joinery_message"), (a, b, e.to_string())));
                }
            }
        } else {
            let open = ui.data(|d| d.get_temp::<bool>(another)).unwrap_or(false);
            let header = egui::CollapsingHeader::new("Add another joint").id_salt(another).open(open.then_some(true));
            let shown = header.show(ui, |ui| picker(ui, doc, (a, b), &fit));
            if shown.header_response.clicked() || open {
                ui.data_mut(|d| d.insert_temp(another, false));
            }
        }
    }
    show_message(ui, (a, b));
}

/// The joint picker: a joint, its sizes and the button that makes it.
fn picker(ui: &mut egui::Ui, doc: &mut Document, (a, b): (InstanceId, InstanceId), fit: &Fit) {
    let name = |doc: &Document, id: InstanceId| doc.instance_name(id).unwrap_or("Part").to_owned();
    let fit = fit.clone();
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
            Choice::LetIn
            | Choice::MortiseAndTenon
            | Choice::Dowels
            | Choice::Biscuits
            | Choice::LooseTenons
            | Choice::PocketScrews
            | Choice::Dovetails
            | Choice::Splines => {
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
                    Choice::Dovetails => {
                        let (_, wide) = fit.contact;
                        let blind_id = egui::Id::new(("half_blind", a, b));
                        let mut half_blind = ui.data(|d| d.get_temp::<bool>(blind_id)).unwrap_or(false);
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut half_blind, false, "Through").on_hover_text("The tails show on both faces of the corner");
                            ui.selectable_value(&mut half_blind, true, "Half-blind").on_hover_text("Hidden from the front, as on a drawer front");
                        });
                        ui.data_mut(|d| d.insert_temp(blind_id, half_blind));
                        let [tails] = fields(ui, ("dovetails", a, b), [("Tails", f64::from(Dovetails::suggested(wide)), "How many tails across the width")]);
                        ui.weak(format!("{pn} gets the tails, {inn} the pins (1:8 slope)."));
                        if let (true, Some(tails)) = (button(ui, tails.is_some(), "Cut the dovetails"), tails) {
                            let style = Dovetails { tails: tails.round().max(0.0) as u32, half_blind };
                            message = Some(result(doc.dovetails(part, into, style).map(|()| format!("Dovetails cut; {pn} is longer by the tails."))));
                        }
                    }
                    Choice::Splines => {
                        let spline = Spline::for_stock(fit.contact.0);
                        ui.weak(format!("A {} along the whole joint, glued into a groove in each part.", spline.piece));
                        if button(ui, true, "Cut the spline groove") {
                            message = Some(result(doc.slots(part, into, &spline).map(|()| "Spline groove cut in both parts.".to_owned())));
                        }
                    }
                    Choice::Biscuits | Choice::LooseTenons => {
                        let (thick, wide) = fit.contact;
                        let biscuits = choice == Choice::Biscuits;
                        let size_id = egui::Id::new(("slot_size", biscuits, a, b));
                        let suggested = if biscuits { Slots::suggested_biscuit(thick, wide) } else { Slots::suggested_loose_tenon(thick) };
                        let mut size = ui.data(|d| d.get_temp::<usize>(size_id)).unwrap_or(suggested);
                        let names: Vec<String> = if biscuits {
                            BISCUITS.iter().map(|b| b.0.to_owned()).collect()
                        } else {
                            LOOSE_TENONS.iter().map(|(t, w, l)| format!("{t} x {w} x {l} mm")).collect()
                        };
                        ui.horizontal(|ui| {
                            ui.label("Size");
                            egui::ComboBox::from_id_salt(size_id).selected_text(&names[size]).show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    ui.selectable_value(&mut size, i, n);
                                }
                            });
                        });
                        ui.data_mut(|d| d.insert_temp(size_id, size));
                        let mut slots = if biscuits { Slots::biscuits(size, wide) } else { Slots::loose_tenons(size, wide) };
                        let [count] = fields(ui, ("slot_count", a, b), [("Count", f64::from(slots.count), "Spread evenly across the joint")]);
                        let label = if biscuits { "Cut biscuit slots" } else { "Cut loose tenon mortises" };
                        if let (true, Some(count)) = (button(ui, count.is_some(), label), count) {
                            slots.count = count.round().max(0.0) as u32;
                            message = Some(result(doc.slots(part, into, &slots).map(|()| format!("{} slots cut in each part, for {} x {}.", slots.count, slots.count, slots.piece))));
                        }
                    }
                    Choice::PocketScrews => {
                        let (thick, wide) = fit.contact;
                        let [count, screw] = fields(
                            ui,
                            ("pocket", a, b),
                            [
                                ("Count", ((wide / 4.0).round()).max(2.0), "About one every 4\""),
                                ("Screw", pocket_screw_for(thick), "Length, for the thickness of the part they go through"),
                            ],
                        );
                        ui.weak(format!("Drilled from the hidden face of {pn}; the holes aren't drawn."));
                        if let (true, Some(count), Some(screw)) = (button(ui, count.is_some() && screw.is_some(), "Add pocket screws"), count, screw) {
                            let n = count.round().max(0.0) as u32;
                            message = Some(result(doc.pocket_screws(part, into, n, screw).map(|()| format!("{n} pocket screws added."))));
                        }
                    }
                    Choice::MortiseAndTenon => {
                        let t = fit.tenon;
                        // Through: the tenon goes all the way and shows on the far side.
                        let through_id = egui::Id::new(("through", a, b));
                        let mut through = ui.data(|d| d.get_temp::<bool>(through_id)).unwrap_or(false);
                        ui.checkbox(&mut through, format!("Through tenon ({} long, shows on the far side)", format_length(fit.depth)));
                        ui.data_mut(|d| d.insert_temp(through_id, through));
                        // Haunch: a short stub filling the shoulder at the end, for a panel groove there.
                        let haunch_id = egui::Id::new(("haunch", a, b));
                        let mut haunched = ui.data(|d| d.get_temp::<bool>(haunch_id)).unwrap_or(false);
                        ui.checkbox(&mut haunched, "Haunch (fills a panel groove at the end)");
                        ui.data_mut(|d| d.insert_temp(haunch_id, haunched));
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
                            let length = if through { fit.depth } else { length };
                            let haunch = if haunched { 0.375f64.min(length / 2.0) } else { 0.0 };
                            message = Some(result(doc.mortise_and_tenon(part, into, Tenon { length, thickness, shoulder, haunch }).map(|()| {
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

    if let Some(m) = message {
        ui.data_mut(|d| d.insert_temp(egui::Id::new("joinery_message"), (a, b, m)));
    }
}

/// What the last joint made (or why not), for this pair.
fn show_message(ui: &mut egui::Ui, (a, b): (InstanceId, InstanceId)) {
    if let Some((ma, mb, m)) = ui.data(|d| d.get_temp::<(InstanceId, InstanceId, String)>(egui::Id::new("joinery_message")))
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
pub fn part_joints(ui: &mut egui::Ui, doc: &mut Document, part: InstanceId) {
    let joints = doc.joint_list(part);
    if joints.is_empty() {
        return;
    }
    ui.label("Joints");
    let mut remove = None;
    for (index, joint, other) in joints {
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!("  {} with {other}, {}", joint.kind.name(), joint.detail));
            if ui.small_button("Remove").on_hover_text("Take this joint out of both parts").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove
        && let Err(e) = doc.remove_joint(index)
    {
        ui.data_mut(|d| d.insert_temp(egui::Id::new(("part_joint_error", part)), e.to_string()));
    }
    if let Some(error) = ui.data(|d| d.get_temp::<String>(egui::Id::new(("part_joint_error", part)))) {
        ui.colored_label(ui.visuals().warn_fg_color, error);
    }
}
