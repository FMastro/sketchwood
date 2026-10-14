//! Cutting diagrams: how to cut the cut list's parts from standard stock with little waste.
//!
//! - Sheet goods: parts are packed onto sheets (4' x 8' by default) with a 2D guillotine packer,
//!   so every layout can be cut with edge-to-edge cuts on a table saw or track saw. Parts only
//!   turn sideways if the material has no grain.
//! - Solid wood: the same, on boards of a standard width and length.
//! - Dimensional lumber: parts go on the smallest standard section that fits (2x4, 1x6, ...)
//!   and are packed by length onto standard lengths (first-fit decreasing); each board is then
//!   shortened to the shortest standard length that still holds its parts.
//!
//! Every cut takes a saw kerf. Several part orders and placement rules are tried and the plan
//! using the least stock wins. These are good heuristics, not guaranteed optima.

use serde::{Deserialize, Serialize};

use crate::cutlist::MaterialParts;
use crate::{MaterialKind, Scene};
use crate::units::format_length;

/// Stock sizes and the saw kerf, in inches. US defaults. Saved with the drawing; a material
/// can override the sizes that apply to its type ([`StockOverride`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub kerf: f64,
    /// Sheet goods: (length, width).
    pub sheet: (f64, f64),
    /// Solid wood boards.
    pub board_lengths: Vec<f64>,
    pub board_widths: Vec<f64>,
    /// Dimensional lumber.
    pub lumber_lengths: Vec<f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            kerf: 0.125,
            sheet: (96.0, 48.0),
            board_lengths: vec![72.0, 96.0, 120.0],
            board_widths: vec![3.5, 5.5, 7.25, 9.25, 11.25],
            lumber_lengths: vec![96.0, 120.0, 144.0, 192.0],
        }
    }
}


/// A material's own stock sizes, replacing the drawing's for its type: the sheet size for
/// sheet goods, board lengths and widths for solid wood, lengths for dimensional lumber.
/// `None` uses the drawing's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StockOverride {
    pub sheet: Option<(f64, f64)>,
    pub lengths: Option<Vec<f64>>,
    pub widths: Option<Vec<f64>>,
}

impl StockOverride {
    pub fn is_empty(&self) -> bool {
        self.sheet.is_none() && self.lengths.is_none() && self.widths.is_none()
    }
}

impl Settings {
    /// These settings with a material's own stock sizes swapped in.
    pub fn with_override(&self, kind: MaterialKind, stock: &StockOverride) -> Settings {
        let mut s = self.clone();
        match kind {
            MaterialKind::SheetGood => s.sheet = stock.sheet.unwrap_or(s.sheet),
            MaterialKind::SolidWood => {
                s.board_lengths = stock.lengths.clone().unwrap_or(s.board_lengths);
                s.board_widths = stock.widths.clone().unwrap_or(s.board_widths);
            }
            MaterialKind::DimensionalLumber => s.lumber_lengths = stock.lengths.clone().unwrap_or(s.lumber_lengths),
            _ => {}
        }
        s
    }
}

/// Plan every material in the drawing's cut list with the drawing's settings and each
/// material's own stock sizes.
pub fn plan_all(scene: &Scene, list: &crate::cutlist::CutList) -> Vec<Option<Plan>> {
    list.materials
        .iter()
        .map(|parts| {
            let settings = match (parts.kind, parts.material.and_then(|m| scene.material(m))) {
                (Some(kind), Some(material)) => scene.cut_settings.with_override(kind, &material.stock),
                _ => scene.cut_settings.clone(),
            };
            plan(parts, &settings)
        })
        .collect()
}
/// Dimensional lumber's actual sizes, with nominal names (a 2x4 is 1.5" x 3.5").
const LUMBER_THICKNESSES: [(f64, u32); 3] = [(0.75, 1), (1.5, 2), (3.5, 4)];
const LUMBER_WIDTHS: [(f64, u32); 7] = [(1.5, 2), (2.5, 3), (3.5, 4), (5.5, 6), (7.25, 8), (9.25, 10), (11.25, 12)];

/// A part placed on a piece of stock. `x` runs along the stock's length, `y` across its width.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub label: String,
    pub x: f64,
    pub y: f64,
    /// Size along the stock's length and width (swapped from the part's if `rotated`).
    pub length: f64,
    pub width: f64,
    /// Turned crossways (only for materials without grain).
    pub rotated: bool,
}

/// One piece of stock to buy, and what to cut from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Stock {
    /// E.g. `Sheet 96" x 48"`, `2x4 x 96"`.
    pub description: String,
    pub length: f64,
    pub width: f64,
    pub placements: Vec<Placement>,
}

impl Stock {
    /// Fraction of the stock's area that ends up in parts.
    pub fn used(&self) -> f64 {
        let parts: f64 = self.placements.iter().map(|p| p.length * p.width).sum();
        parts / (self.length * self.width)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub stocks: Vec<Stock>,
    /// Parts no stock size can hold, with the reason (e.g. "wider than any board: glue up").
    pub unplaced: Vec<(String, String)>,
}

impl Plan {
    /// What to buy: (description, count), in plan order.
    pub fn shopping_list(&self) -> Vec<(String, usize)> {
        let mut list: Vec<(String, usize)> = Vec::new();
        for stock in &self.stocks {
            match list.iter_mut().find(|(d, _)| *d == stock.description) {
                Some((_, n)) => *n += 1,
                None => list.push((stock.description.clone(), 1)),
            }
        }
        list
    }

    /// Fraction of all bought stock that ends up in parts.
    pub fn yield_fraction(&self) -> f64 {
        let stock: f64 = self.stocks.iter().map(|s| s.length * s.width).sum();
        let parts: f64 = self.stocks.iter().flat_map(|s| &s.placements).map(|p| p.length * p.width).sum();
        if stock > 0.0 { parts / stock } else { 0.0 }
    }
}

/// A part to place: one per piece (quantities expanded).
#[derive(Clone, Debug)]
struct Piece {
    label: String,
    length: f64,
    width: f64,
}

/// Plan the cuts for one material, or `None` if it isn't cut from stock (hardware, other, no
/// material).
pub fn plan(material: &MaterialParts, settings: &Settings) -> Option<Plan> {
    let pieces = || {
        material.parts.iter().flat_map(|p| {
            std::iter::repeat_n(Piece { label: p.label.clone(), length: p.length, width: p.width }, p.count)
        })
    };
    match material.kind? {
        MaterialKind::SheetGood => {
            let (length, width) = settings.sheet;
            let name = format!("Sheet {} x {}", format_length(length), format_length(width));
            Some(pack_2d(pieces().collect(), &[(length, width, name)], !material.grained, settings.kerf))
        }
        MaterialKind::SolidWood => {
            // Boards wide and long enough for the biggest part, from the standard sizes.
            let widest = material.parts.iter().map(|p| p.width).fold(0.0, f64::max);
            let longest = material.parts.iter().map(|p| p.length).fold(0.0, f64::max);
            let width = smallest_at_least(&settings.board_widths, widest)
                .or_else(|| settings.board_widths.iter().copied().reduce(f64::max))?;
            let mut sizes: Vec<(f64, f64, String)> = settings
                .board_lengths
                .iter()
                .filter(|&&l| l >= longest)
                .map(|&l| (l, width, format!("Board {} x {}", format_length(width), format_length(l))))
                .collect();
            if sizes.is_empty() {
                let l = settings.board_lengths.iter().copied().fold(0.0, f64::max);
                sizes.push((l, width, format!("Board {} x {}", format_length(width), format_length(l))));
            }
            let mut plan = pack_2d(pieces().collect(), &sizes, false, settings.kerf);
            for (label, reason) in &mut plan.unplaced {
                if material.parts.iter().any(|p| p.label == *label && p.width > width) {
                    *reason = "wider than the widest board: glue up a panel".into();
                }
            }
            Some(plan)
        }
        MaterialKind::DimensionalLumber => Some(plan_lumber(pieces().collect(), material, settings)),
        MaterialKind::Hardware | MaterialKind::Other => None,
    }
}

fn smallest_at_least(sizes: &[f64], at_least: f64) -> Option<f64> {
    sizes.iter().copied().filter(|&s| s >= at_least - 1e-9).reduce(f64::min)
}

/// Lumber: pick a section per part, then pack each section by length.
fn plan_lumber(pieces: Vec<Piece>, material: &MaterialParts, settings: &Settings) -> Plan {
    let mut plan = Plan::default();
    let mut by_section: Vec<(Section, Section, Vec<Piece>)> = Vec::new();
    for piece in pieces {
        let thickness =
            material.parts.iter().find(|p| p.label == piece.label).map_or(material.stock_thickness.unwrap_or(1.5), |p| p.thickness);
        let section = (
            LUMBER_THICKNESSES.iter().copied().find(|(t, _)| *t >= thickness - 1e-9),
            LUMBER_WIDTHS.iter().copied().find(|(w, _)| *w >= piece.width - 1e-9),
        );
        let (Some(t), Some(w)) = section else {
            plan.unplaced.push((piece.label, "bigger than any standard lumber".into()));
            continue;
        };
        match by_section.iter_mut().find(|(st, sw, _)| *st == t && *sw == w) {
            Some((_, _, list)) => list.push(piece),
            None => by_section.push((t, w, vec![piece])),
        }
    }
    for ((_, nominal_t), (width, nominal_w), pieces) in by_section {
        let name = format!("{nominal_t}x{nominal_w}");
        let section_plan = pack_1d(pieces, &settings.lumber_lengths, width, &name, settings.kerf);
        plan.stocks.extend(section_plan.stocks);
        plan.unplaced.extend(section_plan.unplaced);
    }
    plan
}

/// 1D: pieces by length onto stock lengths. See [`best_bins`]; then any board whose parts fit
/// the same or less total length on shorter boards is split (two 8' boards rather than a 16').
fn pack_1d(pieces: Vec<Piece>, lengths: &[f64], width: f64, section: &str, kerf: f64) -> Plan {
    let mut lengths = lengths.to_vec();
    lengths.sort_by(f64::total_cmp);
    let longest_stock = lengths.last().copied().unwrap_or(0.0);
    let (fits, too_long): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(|p| p.length <= longest_stock + 1e-9);
    let bins = best_bins(&fits, &lengths, kerf).map(|(_, bins)| bins).unwrap_or_default();

    let mut boards: Vec<Vec<Piece>> = Vec::new();
    for bin in bins {
        let bought = shortest_holding(&lengths, used_length(&bin, kerf), kerf);
        let shorter: Vec<f64> = lengths.iter().copied().filter(|&l| l < bought - 1e-9).collect();
        match best_bins(&bin, &shorter, kerf) {
            Some(((total, _), split)) if total <= bought + 1e-9 => boards.extend(split),
            _ => boards.push(bin),
        }
    }

    let mut plan = Plan::default();
    for pieces in boards {
        let length = shortest_holding(&lengths, used_length(&pieces, kerf), kerf);
        let mut x = 0.0;
        let placements = pieces
            .into_iter()
            .map(|p| {
                let placement = Placement { label: p.label, x, y: 0.0, length: p.length, width: p.width.min(width), rotated: false };
                x += p.length + kerf;
                placement
            })
            .collect();
        plan.stocks.push(Stock { description: format!("{section} x {}", format_length(length)), length, width, placements });
    }
    plan.unplaced.extend(too_long.into_iter().map(|p| (p.label, "longer than the longest stock".to_owned())));
    plan
}

/// A lumber dimension: (actual inches, nominal size), e.g. (3.5, 4).
type Section = (f64, u32);

/// A 1D packing: ((total length bought, longest board), the pieces on each board).
type Boards = ((f64, f64), Vec<Vec<Piece>>);

/// Pieces plus a kerf after each.
fn used_length(pieces: &[Piece], kerf: f64) -> f64 {
    pieces.iter().map(|p| p.length + kerf).sum()
}

/// Fill boards of each standard length (that holds the longest piece) with first-fit and
/// best-fit decreasing, shorten every board to the shortest length that holds it, and keep the
/// packing buying the least total length; ties go to shorter boards, which are easier to carry
/// and usually cheaper per foot. Returns ((total length, longest board), boards), or `None` if
/// no length holds the longest piece. `lengths` must be sorted.
fn best_bins(pieces: &[Piece], lengths: &[f64], kerf: f64) -> Option<Boards> {
    let longest_piece = pieces.iter().map(|p| p.length).fold(0.0, f64::max);
    let mut best: Option<Boards> = None;
    let capacities = lengths.iter().copied().filter(|&l| l >= longest_piece - 1e-9);
    for (capacity, best_fit) in capacities.flat_map(|c| [(c, false), (c, true)]) {
        let mut sorted = pieces.to_vec();
        sorted.sort_by(|a, b| b.length.total_cmp(&a.length));
        // Boards of pieces, each with its used length (pieces plus a kerf after each).
        let mut bins: Vec<(f64, Vec<Piece>)> = Vec::new();
        let room = |used: f64| capacity + kerf - used;
        for piece in sorted {
            let need = piece.length + kerf;
            let choice = if best_fit {
                bins.iter()
                    .enumerate()
                    .filter(|(_, (u, _))| room(*u) >= need - 1e-9)
                    .min_by(|a, b| room(a.1.0).total_cmp(&room(b.1.0)))
                    .map(|(i, _)| i)
            } else {
                bins.iter().position(|(u, _)| room(*u) >= need - 1e-9)
            };
            match choice {
                Some(i) => {
                    bins[i].0 += need;
                    bins[i].1.push(piece);
                }
                None => bins.push((need, vec![piece])),
            }
        }
        let bought: Vec<f64> = bins.iter().map(|(used, _)| shortest_holding(lengths, *used, kerf)).collect();
        let cost = (bought.iter().sum::<f64>(), bought.iter().copied().fold(0.0, f64::max));
        let better = best.as_ref().is_none_or(|((total, longest), _)| {
            cost.0 < total - 1e-9 || ((cost.0 - total).abs() <= 1e-9 && cost.1 < longest - 1e-9)
        });
        if better {
            best = Some((cost, bins.into_iter().map(|(_, p)| p).collect()));
        }
    }
    best
}

/// The shortest stock length whose parts plus kerfs (the last kerf isn't needed) fit.
fn shortest_holding(lengths: &[f64], used_with_kerfs: f64, kerf: f64) -> f64 {
    lengths.iter().copied().find(|&l| l + kerf >= used_with_kerfs - 1e-9).unwrap_or_else(|| lengths.last().copied().unwrap_or(0.0))
}

/// A free rectangle on a piece of stock.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    l: f64,
    w: f64,
}

/// How a part is chosen for a free rectangle, and how the leftover is cut.
#[derive(Clone, Copy)]
struct Rule {
    fit: Fit,
    split: Split,
}

#[derive(Clone, Copy)]
enum Fit {
    /// Smallest leftover on the free rectangle's shorter side.
    BestShortSide,
    /// Smallest free rectangle.
    BestArea,
}

/// Which through-cut separates the leftover after placing a part in a free rectangle's corner.
#[derive(Clone, Copy)]
enum Split {
    /// Cut along whichever leftover is shorter, keeping the larger piece of offcut whole.
    ShorterLeftover,
    LongerLeftover,
    /// Always rip along the stock's length first, the usual way to break down sheets.
    RipFirst,
}

/// A piece of stock being filled.
#[derive(Clone)]
struct Bin {
    size: usize,
    free: Vec<Rect>,
    placements: Vec<Placement>,
}

/// 2D: pieces onto stock of the given sizes, (length, width, description). The first size is
/// opened when a new piece of stock is needed; later sizes are used to shrink the last board
/// if they still hold it (boards come in several lengths). Tries several orders and rules.
fn pack_2d(pieces: Vec<Piece>, sizes: &[(f64, f64, String)], can_rotate: bool, kerf: f64) -> Plan {
    let Some(&(length, width, _)) = sizes.last() else { return Plan::default() };
    let fits = |p: &Piece| {
        (p.length <= length + 1e-9 && p.width <= width + 1e-9) || (can_rotate && p.width <= length + 1e-9 && p.length <= width + 1e-9)
    };
    let (pieces, too_big): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(fits);
    let orders: [fn(&Piece, &Piece) -> std::cmp::Ordering; 3] = [
        |a, b| (b.length * b.width).total_cmp(&(a.length * a.width)),
        |a, b| b.length.total_cmp(&a.length).then(b.width.total_cmp(&a.width)),
        |a, b| b.width.total_cmp(&a.width).then(b.length.total_cmp(&a.length)),
    ];
    let mut best: Option<(f64, Vec<Bin>)> = None;
    for order in orders {
        let rules = [Fit::BestShortSide, Fit::BestArea]
            .into_iter()
            .flat_map(|fit| [Split::ShorterLeftover, Split::LongerLeftover, Split::RipFirst].map(|split| Rule { fit, split }));
        for rule in rules {
            let mut sorted = pieces.clone();
            sorted.sort_by(order);
            let bins = fill_bins(sorted, sizes, can_rotate, kerf, rule);
            let bought: f64 = bins.iter().map(|b| sizes[b.size].0 * sizes[b.size].1).sum();
            if best.as_ref().is_none_or(|(b, _)| bought < *b - 1e-9) {
                best = Some((bought, bins));
            }
        }
    }
    let mut plan = Plan::default();
    for bin in best.map(|(_, bins)| bins).unwrap_or_default() {
        let (length, width, description) = sizes[bin.size].clone();
        plan.stocks.push(Stock { description, length, width, placements: bin.placements });
    }
    plan.unplaced = too_big.into_iter().map(|p| (p.label, "bigger than the stock".to_owned())).collect();
    plan
}

fn fill_bins(pieces: Vec<Piece>, sizes: &[(f64, f64, String)], can_rotate: bool, kerf: f64, rule: Rule) -> Vec<Bin> {
    // Always open the biggest stock; boards get shortened afterwards.
    let biggest = sizes.len() - 1;
    let open = |size: usize| {
        let (l, w, _) = sizes[size];
        // Each part takes a kerf on its far sides, so the stock is effectively a kerf bigger.
        Bin { size, free: vec![Rect { x: 0.0, y: 0.0, l: l + kerf, w: w + kerf }], placements: Vec::new() }
    };
    let mut bins: Vec<Bin> = Vec::new();
    for piece in pieces {
        let placed = bins.iter_mut().any(|bin| place(bin, &piece, can_rotate, kerf, rule));
        if !placed {
            let mut bin = open(biggest);
            if place(&mut bin, &piece, can_rotate, kerf, rule) {
                bins.push(bin);
            }
        }
    }
    // Shorten each board to the smallest size that still holds its parts.
    for bin in &mut bins {
        let reach_l = bin.placements.iter().map(|p| p.x + p.length).fold(0.0, f64::max);
        let reach_w = bin.placements.iter().map(|p| p.y + p.width).fold(0.0, f64::max);
        if let Some(size) = (0..sizes.len()).find(|&s| sizes[s].0 >= reach_l - 1e-9 && sizes[s].1 >= reach_w - 1e-9) {
            bin.size = size;
        }
    }
    bins
}

/// Place a piece in a corner of one of a bin's free rectangles, if it fits, then split that
/// rectangle's leftover with one straight through-cut (guillotine packing). Free rectangles never
/// overlap, so every layout can be cut on a table saw or track saw with edge-to-edge cuts.
fn place(bin: &mut Bin, piece: &Piece, can_rotate: bool, kerf: f64, rule: Rule) -> bool {
    let orientations: &[bool] = if can_rotate { &[false, true] } else { &[false] };
    let mut best: Option<((f64, f64), usize, bool)> = None;
    for &rotated in orientations {
        let (l, w) = if rotated { (piece.width, piece.length) } else { (piece.length, piece.width) };
        let (need_l, need_w) = (l + kerf, w + kerf);
        for (i, free) in bin.free.iter().enumerate() {
            if free.l + 1e-9 < need_l || free.w + 1e-9 < need_w {
                continue;
            }
            let (left_l, left_w) = (free.l - need_l, free.w - need_w);
            let score = match rule.fit {
                Fit::BestShortSide => (left_l.min(left_w), left_l.max(left_w)),
                Fit::BestArea => (free.l * free.w - need_l * need_w, left_l.min(left_w)),
            };
            if best.as_ref().is_none_or(|(s, ..)| score < *s) {
                best = Some((score, i, rotated));
            }
        }
    }
    let Some((_, index, rotated)) = best else { return false };
    let free = bin.free.swap_remove(index);
    let (length, width) = if rotated { (piece.width, piece.length) } else { (piece.length, piece.width) };
    let (need_l, need_w) = (length + kerf, width + kerf);
    let (left_l, left_w) = (free.l - need_l, free.w - need_w);
    // Cutting "along" runs the full length of the free rectangle (a rip on the stock).
    let along = match rule.split {
        Split::ShorterLeftover => left_w < left_l,
        Split::LongerLeftover => left_w >= left_l,
        Split::RipFirst => true,
    };
    let (beside, beyond) = if along {
        // Rip off the strip holding the part, then crosscut the strip after the part.
        (Rect { x: free.x + need_l, y: free.y, l: left_l, w: need_w }, Rect { x: free.x, y: free.y + need_w, l: free.l, w: left_w })
    } else {
        // Crosscut off the part's column, then cut the column above the part.
        (Rect { x: free.x + need_l, y: free.y, l: left_l, w: free.w }, Rect { x: free.x, y: free.y + need_w, l: need_l, w: left_w })
    };
    bin.free.extend([beside, beyond].into_iter().filter(|r| r.l > 1e-9 && r.w > 1e-9));
    bin.placements.push(Placement { label: piece.label.clone(), x: free.x, y: free.y, length, width, rotated });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    use crate::cutlist::Part;

    fn material(kind: MaterialKind, grained: bool, parts: &[(&str, usize, f64, f64, f64)]) -> MaterialParts {
        MaterialParts {
            material: None,
            name: "Test".into(),
            kind: Some(kind),
            grained,
            stock_thickness: None,
            parts: parts
                .iter()
                .map(|&(label, count, length, width, thickness)| Part {
                    label: label.into(),
                    names: vec![],
                    count,
                    length,
                    width,
                    thickness,
                })
                .collect(),
        }
    }

    /// Whether parts (x, y, length, width, each including its kerf) can be separated by
    /// straight edge-to-edge cuts, recursively, as a table saw or track saw requires.
    fn guillotine(parts: &[[f64; 4]]) -> bool {
        if parts.len() <= 1 {
            return true;
        }
        for axis in [0, 1] {
            for p in parts {
                let cut = p[axis] + p[axis + 2];
                let (before, after): (Vec<[f64; 4]>, Vec<[f64; 4]>) =
                    parts.iter().partition(|r| r[axis] + r[axis + 2] <= cut + 1e-9);
                let clean = after.iter().all(|r| r[axis] >= cut - 1e-9);
                if clean && !before.is_empty() && !after.is_empty() && guillotine(&before) && guillotine(&after) {
                    return true;
                }
            }
        }
        false
    }

    /// No two parts on a piece of stock overlap (with kerf), all stay on the stock, and the
    /// layout can be cut with edge-to-edge cuts.
    fn assert_valid(plan: &Plan, kerf: f64) {
        for stock in &plan.stocks {
            let rects: Vec<[f64; 4]> = stock.placements.iter().map(|p| [p.x, p.y, p.length + kerf, p.width + kerf]).collect();
            assert!(guillotine(&rects), "{} can't be cut with through-cuts: {:?}", stock.description, stock.placements);
            for (i, a) in stock.placements.iter().enumerate() {
                assert!(a.x >= -1e-9 && a.y >= -1e-9, "{a:?} off the stock");
                assert!(a.x + a.length <= stock.length + 1e-9, "{a:?} past the end of {}", stock.description);
                assert!(a.y + a.width <= stock.width + 1e-9, "{a:?} past the edge of {}", stock.description);
                for b in &stock.placements[i + 1..] {
                    let apart = a.x + a.length + kerf <= b.x + 1e-9
                        || b.x + b.length + kerf <= a.x + 1e-9
                        || a.y + a.width + kerf <= b.y + 1e-9
                        || b.y + b.width + kerf <= a.y + 1e-9;
                    assert!(apart, "{a:?} and {b:?} overlap on {}", stock.description);
                }
            }
        }
    }

    #[test]
    fn a_materials_own_sheet_size_replaces_the_drawings() {
        use crate::{Document, Entity};
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 50.0, DVec3::Y * 30.0, DVec3::Z)).unwrap();
        let panel = doc.push_pull(face, 0.75, true).unwrap().unwrap();
        let ply = doc.add_material(MaterialKind::SheetGood);
        doc.paint(&[Entity::Instance(panel)], Some(ply));
        let sheets = |doc: &Document| {
            let list = crate::cutlist::cut_list(doc.scene());
            plan_all(doc.scene(), &list)[0].as_ref().unwrap().shopping_list()[0].0.clone()
        };
        assert_eq!(sheets(&doc), "Sheet 96\" x 48\"");
        // Baltic birch comes 5' x 5'.
        doc.edit_material(ply, |m| m.stock.sheet = Some((60.0, 60.0)));
        assert_eq!(sheets(&doc), "Sheet 60\" x 60\"");
        // Saved with the drawing, as are the drawing's own settings.
        doc.set_cut_settings(Settings { kerf: 0.09375, ..Settings::default() });
        let reloaded = crate::file::from_str(&crate::file::to_string(doc.scene())).unwrap();
        assert_eq!(reloaded.cut_settings.kerf, 0.09375);
        assert_eq!(reloaded.material(ply).unwrap().stock.sheet, Some((60.0, 60.0)));
    }

    #[test]
    fn older_files_get_the_default_stock() {
        let text = r#"{"format":"sketchcraft","version":1,"units":"inches","scene":{"root":{"model":{"vertices":[],"edges":[],"faces":[]},"instances":[]},"definitions":[],"tags":[],"materials":[],"next_group":0,"next_component":0}}"#;
        let scene = crate::file::from_str(text).unwrap();
        assert_eq!(scene.cut_settings, Settings::default());
    }

    #[test]
    fn cabinet_sides_fit_two_sheets() {
        // Two cabinet carcasses: 4 sides 30" x 23.25", 2 bottoms and 2 tops 34.5" x 23.25",
        // 2 backs 34.5" x 30".
        let m = material(
            MaterialKind::SheetGood,
            true,
            &[("A", 4, 30.0, 23.25, 0.75), ("B", 4, 34.5, 23.25, 0.75), ("C", 2, 34.5, 30.0, 0.75)],
        );
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        assert!(plan.unplaced.is_empty());
        let placed: usize = plan.stocks.iter().map(|s| s.placements.len()).sum();
        assert_eq!(placed, 10);
        // Parts are 7059 sq in; a sheet is 4608: at least 2 sheets, and no more than 3.
        assert!((2..=3).contains(&plan.stocks.len()), "{} sheets", plan.stocks.len());
        assert_eq!(plan.shopping_list()[0].0, "Sheet 96\" x 48\"");
        // Grained: nothing turned sideways.
        assert!(plan.stocks.iter().flat_map(|s| &s.placements).all(|p| !p.rotated));
    }

    #[test]
    fn ungrained_sheets_turn_parts_to_fit() {
        // 46" x 20" parts on a grained sheet fit 2 across the width, 2 along: 4 per sheet. On
        // MDF (no grain) the same holds; but a 40" x 47" part only fits crossways... it fits
        // either way here, so check a part that only fits rotated.
        let settings = Settings { sheet: (60.0, 30.0), ..Settings::default() };
        let crossways = material(MaterialKind::SheetGood, false, &[("A", 1, 28.0, 50.0, 0.5)]);
        let plan = plan(&crossways, &settings).unwrap();
        assert!(plan.unplaced.is_empty());
        assert!(plan.stocks[0].placements[0].rotated);
        let grained = material(MaterialKind::SheetGood, true, &[("A", 1, 28.0, 50.0, 0.5)]);
        let plan = super::plan(&grained, &settings).unwrap();
        assert_eq!(plan.unplaced.len(), 1, "with grain it can't turn, so it doesn't fit");
    }

    #[test]
    fn studs_go_on_the_shortest_lumber_that_holds_them() {
        // 2x4s: three 46" pieces and one 92 5/8" stud.
        let m = material(MaterialKind::DimensionalLumber, true, &[("A", 1, 92.625, 3.5, 1.5), ("B", 3, 46.0, 3.5, 1.5)]);
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        // The stud plus one 46" piece fits a 12' board (92.75 + 46.125 = 138.875"), and the
        // other two share an 8': 240", less than three 8' boards (288").
        assert_eq!(plan.shopping_list(), vec![("2x4 x 144\"".to_owned(), 1), ("2x4 x 96\"".to_owned(), 1)]);
        // With only 8' and 16' boards, a 16' + 8' and three 8's both buy 288"; the tie goes to
        // the shorter boards.
        let settings = Settings { lumber_lengths: vec![96.0, 192.0], ..Settings::default() };
        assert_eq!(super::plan(&m, &settings).unwrap().shopping_list(), vec![("2x4 x 96\"".to_owned(), 3)]);
    }

    #[test]
    fn lumber_picks_the_smallest_section_and_packs_by_length() {
        // Ten 30" 1x6 slats (0.75 x 5.5). On 8' boards: 3 each (3 x 30.125 = 90.375), so four
        // boards, 384". A 16' board holds 6 and a 12' the other 4 (120.5"): 336", less lumber.
        let m = material(MaterialKind::DimensionalLumber, true, &[("A", 10, 30.0, 5.5, 0.75)]);
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        assert!(plan.stocks.iter().all(|s| s.description.starts_with("1x6")));
        let bought: f64 = plan.stocks.iter().map(|s| s.length).sum();
        assert_eq!(bought, 336.0);
        // Only 8' boards on hand: four of them.
        let eights = Settings { lumber_lengths: vec![96.0], ..Settings::default() };
        assert_eq!(super::plan(&m, &eights).unwrap().shopping_list(), vec![("1x6 x 96\"".to_owned(), 4)]);
    }

    #[test]
    fn long_boards_split_into_shorter_ones_when_it_costs_nothing() {
        // Two 92 5/8" studs and three 46" rails. Two studs fill a 16' board, but two 8' boards
        // are the same length and easier to carry; the rails share a 12' board.
        let m = material(MaterialKind::DimensionalLumber, true, &[("A", 2, 92.625, 3.5, 1.5), ("B", 3, 46.0, 3.5, 1.5)]);
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        let mut buy = plan.shopping_list();
        buy.sort();
        assert_eq!(buy, vec![("2x4 x 144\"".to_owned(), 1), ("2x4 x 96\"".to_owned(), 2)]);
    }

    #[test]
    fn oversize_parts_are_reported() {
        let m = material(MaterialKind::DimensionalLumber, true, &[("A", 1, 300.0, 3.5, 1.5)]);
        let plan = plan(&m, &Settings::default()).unwrap();
        assert!(plan.stocks.is_empty());
        assert_eq!(plan.unplaced, vec![("A".to_owned(), "longer than the longest stock".to_owned())]);
        let wide = material(MaterialKind::SolidWood, true, &[("B", 1, 30.0, 18.0, 0.75)]);
        let plan = super::plan(&wide, &Settings::default()).unwrap();
        assert!(plan.unplaced[0].1.contains("glue up"));
    }

    #[test]
    fn solid_wood_uses_boards_wide_enough_and_shortens_them() {
        // Four 20" x 5" table aprons: on 5 1/2" boards, 4 fit end to end on one board
        // (4 x 20.125 = 80.5 <= 96), so a single board, cut to the shortest length holding it.
        let m = material(MaterialKind::SolidWood, true, &[("A", 4, 20.0, 5.0, 0.75)]);
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        assert_eq!(plan.stocks.len(), 1);
        assert_eq!(plan.stocks[0].width, 5.5);
        assert_eq!(plan.stocks[0].length, 96.0);
        let short = material(MaterialKind::SolidWood, true, &[("A", 3, 20.0, 5.0, 0.75)]);
        let plan = super::plan(&short, &settings).unwrap();
        assert_eq!(plan.stocks[0].length, 72.0, "three aprons fit a 6' board");
    }

    #[test]
    fn a_mixed_sheet_job_stays_cuttable_with_through_cuts() {
        // A bookcase's worth of parts on MDF (no grain, so parts may turn).
        let m = material(
            MaterialKind::SheetGood,
            false,
            &[
                ("A", 2, 72.0, 11.25, 0.75),
                ("B", 5, 34.5, 11.25, 0.75),
                ("C", 1, 72.0, 36.0, 0.25),
                ("D", 6, 14.0, 9.0, 0.75),
                ("E", 3, 22.0, 17.5, 0.75),
            ],
        );
        let settings = Settings::default();
        let plan = plan(&m, &settings).unwrap();
        assert_valid(&plan, settings.kerf);
        assert!(plan.unplaced.is_empty());
        assert_eq!(plan.stocks.iter().map(|s| s.placements.len()).sum::<usize>(), 17);
    }

    #[test]
    fn the_cut_checker_rejects_a_pinwheel() {
        // Four parts around a square hole: no straight cut separates them.
        let pinwheel = [[0.0, 0.0, 2.0, 1.0], [2.0, 0.0, 1.0, 2.0], [1.0, 2.0, 2.0, 1.0], [0.0, 1.0, 1.0, 2.0]];
        assert!(!guillotine(&pinwheel));
        assert!(guillotine(&[[0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 1.0, 1.0], [0.0, 1.0, 2.0, 1.0]]));
    }

    #[test]
    fn hardware_has_no_cutting_plan() {
        let m = material(MaterialKind::Hardware, false, &[("A", 8, 2.0, 1.0, 0.1)]);
        assert!(plan(&m, &Settings::default()).is_none());
    }

    #[test]
    fn yield_is_parts_over_stock() {
        let m = material(MaterialKind::SheetGood, true, &[("A", 1, 96.0, 48.0, 0.75)]);
        let settings = Settings { kerf: 0.0, ..Settings::default() };
        let plan = plan(&m, &settings).unwrap();
        assert!((plan.yield_fraction() - 1.0).abs() < 1e-9);
    }
}
