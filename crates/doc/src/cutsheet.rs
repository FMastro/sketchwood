//! The printable cut sheet: a self-contained web page with, for each material, the parts table,
//! what to buy and the cutting diagrams (as SVG, drawn to scale). Opened in a browser, it prints
//! one material per page, or saves as a PDF.

use std::fmt::Write;

use crate::cutlist::CutList;
use crate::cutplan::Plan;
use crate::units::format_length;
use crate::{MaterialKind, Scene};

/// Escape text for HTML element content (never used inside attributes).
fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The cut sheet for `list`, with `plans` from [`crate::cutplan::plan_all`].
pub fn to_html(scene: &Scene, list: &CutList, plans: &[Option<Plan>], title: &str) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<style>
body {{ font: 13px/1.4 system-ui, sans-serif; color: #222; margin: 24px; }}
h1 {{ font-size: 20px; margin: 0 0 4px; }}
h2 {{ font-size: 16px; margin: 0 0 4px; display: flex; align-items: center; gap: 8px; }}
.swatch {{ width: 14px; height: 14px; border: 1px solid #888; display: inline-block; }}
.meta {{ color: #666; margin-bottom: 8px; }}
table {{ border-collapse: collapse; margin: 6px 0 12px; }}
th, td {{ border: 1px solid #ccc; padding: 3px 8px; text-align: left; }}
th {{ background: #f2f2f2; }}
td.num {{ text-align: right; }}
.material {{ margin-bottom: 28px; }}
.stock {{ margin: 6px 0 10px; }}
.warn {{ color: #a33; }}
svg text {{ font: 11px system-ui, sans-serif; }}
@media print {{ .material {{ page-break-after: always; }} .noprint {{ display: none; }} body {{ margin: 0; }} }}
</style></head><body>
<h1>{title}</h1>
<p class="meta noprint"><button onclick="window.print()">Print</button> Each material prints on its own page. Lengths in inches.</p>
"#,
        title = esc(title)
    );
    if list.materials.is_empty() {
        html.push_str("<p>No parts yet. Each group or component is a part.</p>");
    }
    for (material, plan) in list.materials.iter().zip(plans) {
        let color = material.material.and_then(|m| scene.material(m)).map_or([230, 230, 225], |m| m.color);
        let kind = material.kind.map_or("no material", MaterialKind::name);
        let _ = write!(
            html,
            r#"<section class="material"><h2><span class="swatch" style="background:rgb({},{},{})"></span>{}</h2>
<div class="meta">{} · {} parts</div>
<table><tr><th></th><th>Name</th><th>Qty</th><th>Length</th><th>Width</th><th>Thickness</th></tr>
"#,
            color[0],
            color[1],
            color[2],
            esc(&material.name),
            kind,
            material.part_count()
        );
        for part in &material.parts {
            let _ = writeln!(
                html,
                r#"<tr><td><b>{}</b></td><td>{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td></tr>"#,
                esc(&part.label),
                esc(&part.names.join(", ")),
                part.count,
                esc(&format_length(part.length)),
                esc(&format_length(part.width)),
                esc(&format_length(part.thickness))
            );
        }
        html.push_str("</table>\n");
        if let Some(plan) = plan {
            let buy: Vec<String> = plan.shopping_list().into_iter().map(|(what, n)| format!("{n} × {}", esc(&what))).collect();
            let _ = writeln!(html, "<p><b>Buy:</b> {} <span class=\"meta\">({:.0}% used)</span></p>", buy.join(", "), plan.yield_fraction() * 100.0);
            for (label, reason) in &plan.unplaced {
                let _ = writeln!(html, "<p class=\"warn\">Part {}: {}</p>", esc(label), esc(reason));
            }
            for (i, stock) in plan.stocks.iter().enumerate() {
                let _ = writeln!(
                    html,
                    "<div class=\"stock\">{}. {} · {:.0}% used<br>{}</div>",
                    i + 1,
                    esc(&stock.description),
                    stock.used() * 100.0,
                    stock_svg(stock, color)
                );
            }
        }
        html.push_str("</section>\n");
    }
    html.push_str("</body></html>\n");
    html
}

/// One piece of stock to scale (720 px across), its parts labeled.
fn stock_svg(stock: &crate::cutplan::Stock, color: [u8; 3]) -> String {
    let width = 720.0;
    let scale = width / stock.length;
    // Keep long, narrow lumber visible.
    let height = (stock.width * scale).max(24.0);
    let y_scale = height / stock.width;
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="-1 -1 {vw} {vh}"><rect x="0" y="0" width="{width}" height="{height}" fill="#e6e6e6" stroke="#888"/>"##,
        w = width + 2.0,
        h = height + 2.0,
        vw = width + 2.0,
        vh = height + 2.0,
    );
    for p in &stock.placements {
        let (x, y) = (p.x * scale, p.y * y_scale);
        let (w, h) = (p.length * scale, p.width * y_scale);
        let label = if w > 90.0 && h > 28.0 {
            format!(
                "<tspan x=\"{cx}\" dy=\"-0.2em\">{}</tspan><tspan x=\"{cx}\" dy=\"1.2em\">{} x {}</tspan>",
                esc(&p.label),
                esc(&format_length(p.length)),
                esc(&format_length(p.width)),
                cx = x + w / 2.0
            )
        } else {
            esc(&p.label)
        };
        let _ = write!(
            svg,
            r##"<rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="rgb({},{},{})" fill-opacity="0.85" stroke="#333"/><text x="{:.2}" y="{:.2}" text-anchor="middle" dominant-baseline="middle">{label}</text>"##,
            color[0],
            color[1],
            color[2],
            x + w / 2.0,
            y + h / 2.0
        );
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::*;
    use crate::{Document, Entity};

    #[test]
    fn the_sheet_lists_parts_and_draws_the_cuts() {
        let mut doc = Document::default();
        let face = doc.edit(|m| m.add_rectangle(DVec3::ZERO, DVec3::X * 30.0, DVec3::Y * 20.0, DVec3::Z)).unwrap();
        let side = doc.push_pull(face, 0.75, true).unwrap().unwrap();
        doc.rename_instance(side, "Side <left>");
        let ply = doc.add_material(MaterialKind::SheetGood);
        doc.paint(&[Entity::Instance(side)], Some(ply));
        let list = crate::cutlist::cut_list(doc.scene());
        let plans = crate::cutplan::plan_all(doc.scene(), &list);
        let html = to_html(doc.scene(), &list, &plans, "Bench & shelf");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<title>Bench &amp; shelf</title>"));
        assert!(html.contains("Side &lt;left&gt;"), "names are escaped");
        assert!(html.contains("1 × Sheet 96\" x 48\" x 3/4\""), "{html}");
        assert_eq!(html.matches("<svg").count(), 1);
        assert!(html.contains("window.print()"));
    }
}
