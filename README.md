# Fast CSV Plotter ⚡

A high-performance desktop application for opening, exploring, and plotting large facility/sensor CSV files.

Built using:
- **Engine**: **Rust** + **Polars** (extremely fast parallel processing and CSV parsing)
- **GUI**: **egui** (via `eframe` for a highly responsive, immediate-mode GUI)
- **Plotting**: **egui_plot** (for interactive hardware-accelerated 2D line plotting)

---

## Key Features

1. **Lightning Fast Parsing**: Uses Polars multi-threaded CSV parser. Can handle gigabyte-sized CSVs in seconds.
2. **Asynchronous Processing**: CSV loading is performed in a background thread, keeping the GUI responsive (60 FPS) and preventing window freezes.
3. **Smart Downsampling (Decimation)**: Automatically downsamples massive datasets (e.g. over 50,000 points) to a user-defined max density to guarantee fluid panning and zooming.
4. **Interactive Graphing**: Drag to pan, scroll to zoom, double-click to auto-reset view bounds.
5. **Statistical Insights**: Calculates and displays Min, Max, Mean, Standard Deviation, and valid count instantly for selected columns.
6. **Customizable Styling**: Adjust plot line width, color, and downsampling limits directly from the settings panel.

---

## File Structure

```text
fast-csv-plotter/
├── .github/
│   └── workflows/
│       └── build.yml       # GitHub Actions workflow for auto-building Windows .exe
├── src/
│   ├── app.rs              # Core GUI application logic and background parser
│   └── main.rs             # Application bootstrapper and window initializer
├── .gitignore              # Git ignore rules for Rust builds
├── Cargo.toml              # Rust crate dependencies and features configuration
└── README.md               # Documentation
```

---

## How to Build & Run Locally

### Prerequisites
Make sure you have Rust installed on your computer. If not, download it from [rustup.rs](https://rustup.rs/).

### Compilation
To compile and run the application in development mode:
```bash
cargo run
```

To run a fully optimized release build (strongly recommended for loading very large CSV files):
```bash
cargo run --release
```

---

## CI/CD Pipeline (Automatic Windows Executable)

When you push this repository to GitHub, the configured GitHub Actions workflow (`.github/workflows/build.yml`) will automatically:
1. Spin up a Windows runner.
2. Install the stable Rust toolchain.
3. Compile the application in `--release` mode.
4. Upload the final compiled binary (`fast-csv-plotter.exe`) as a build artifact named `fast-csv-plotter-windows`.

You can download your Windows executable directly from the **Actions** tab of your repository.
