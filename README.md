# WoodSketch

**Design it the way you already know.** WoodSketch is a free, open-source 3D modeler for
woodworkers. It works the way SketchUp does, but the parts know they're wood: species, grain,
real stock sizes, joinery, cut lists and build sheets.

**Website and downloads: [woodsketch.net](https://woodsketch.net)**

![Cutting diagrams and a shopping list](https://woodsketch.net/images/cutting-diagrams-shopping-list.webp)

## Download

WoodSketch is in **beta**. Installers for Windows, macOS (Apple silicon and Intel) and Linux are
on [woodsketch.net](https://woodsketch.net/#get-started) and in
[GitHub Releases](https://github.com/FMastro/woodsketch/releases/latest). Installed copies check
for updates when they start and install them when you restart.

The installers aren't signed yet. On Windows, click *More info*, then *Run anyway*; on a Mac,
right-click the installer and choose *Open*.

## What it does

- **Feels like SketchUp.** Select, Line, Rectangle, Circle, Arc, Push/Pull, Offset, Move,
  Rotate, Scale, Tape Measure, Eraser and Paint Bucket, with the same shortcuts. Type sizes
  into the Measurements box as you go (`3/4"`, `2'6`, `38mm`). Inference and snapping work the
  same way.
- **Groups, components and tags**, an Outliner, copy and paste, and arrays (`3x`, `/3`).
- **Boards from real stock.** Pick a material and a standard size (a 2x4, 3/4" plywood, 4/4
  cherry) and drag it out.
- **Templates.** A cabinet, drawer, face frame or table from a few sizes, with its joinery done.
- **Joinery.** Dados and rabbets, mortise and tenon (haunched, through), dowels, biscuits,
  loose tenons, pocket screws, half-laps, box joints, dovetails and splines, cut into the parts
  and listed with them.
- **Edge profiles.** Roundover, chamfer, cove and ogee.
- **Cut list and cutting diagrams.** Parts grouped by material and size, packed onto sheets
  and boards with kerf and grain direction, a shopping list, and glue-ups for wide panels.
- **Build sheets.** One printable page per part with views to scale and the steps in shop
  order.
- **Wood movement checks.** Warns where a part will swell or shrink across its grain against
  a part that won't move with it.
- **Save, autosave and export.** `.woodsketch` files, crash recovery, OBJ and STL export.

![A part showing its grain direction](https://woodsketch.net/images/part-grain-direction.webp)

## Build from source

WoodSketch is written in Rust (egui and wgpu).

```sh
git clone https://github.com/FMastro/woodsketch
cd woodsketch
cargo run --release -p woodsketch
```

On Linux, first install the windowing and file-dialog libraries, for example on Ubuntu:
`sudo apt install libgtk-3-dev libxkbcommon-dev libwayland-dev libxcursor-dev libxrandr-dev libxi-dev libx11-xcb-dev`.

Run the tests with `cargo test --workspace`. `CLAUDE.md` describes how the code is laid out,
and `docs/BACKLOG.md` lists what's planned.

## Feedback

Found a bug, or missing a feature? [Open an issue](https://github.com/FMastro/woodsketch/issues).

WoodSketch reimplements SketchUp's behavior from its public documentation and from using it. It
isn't affiliated with Trimble or SketchUp, and contains none of their code or artwork.

## License

See [LICENSE](LICENSE).
