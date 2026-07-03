//! Mesure le one-shot et le coût marginal d'une itération de recherche.
//! Usage: cargo run --release -p ssimulacra2 --example bench_search

use std::path::PathBuf;
use std::time::Instant;

use ssimulacra2::{ColorPrimaries, Rgb, TransferCharacteristic, compute_frame_ssimulacra2};

fn load_rgb(name: &str) -> Rgb {
    let img = image::open(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(name),
    )
    .unwrap();
    let data = img
        .to_rgb32f()
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect::<Vec<_>>();
    Rgb::new(
        data,
        img.width() as usize,
        img.height() as usize,
        TransferCharacteristic::SRGB,
        ColorPrimaries::BT709,
    )
    .unwrap()
}

fn main() {
    let src = load_rgb("tank_source.png");
    let dst = load_rgb("tank_distorted.png");

    // Échauffement + one-shot moyenné sur 5 runs.
    let mut score = 0.0;
    let t0 = Instant::now();
    for _ in 0..5 {
        score = compute_frame_ssimulacra2(src.clone(), dst.clone()).unwrap();
    }
    println!(
        "one-shot: {:.1} ms (score {score:.4})",
        t0.elapsed().as_secs_f64() * 1000.0 / 5.0
    );

    let t1 = Instant::now();
    let r = ssimulacra2::ReferenceFrame::new(src.clone()).unwrap();
    println!("ref build: {:.1} ms", t1.elapsed().as_secs_f64() * 1000.0);

    let t2 = Instant::now();
    for _ in 0..4 {
        r.score(dst.clone()).unwrap();
    }
    println!(
        "score (ref chaude): {:.1} ms/appel",
        t2.elapsed().as_secs_f64() * 1000.0 / 4.0
    );
}
