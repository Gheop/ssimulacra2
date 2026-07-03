use std::path::PathBuf;

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

// Goldens enregistrés sur le code upstream non modifié (commit de la task 1).
// Toute évolution du fork doit rester à 1e-4 de ces valeurs.
const CASES: &[(&str, &str, f64)] = &[
    ("tank_source.png", "tank_distorted.png", 17.385_284_36),
    ("odd_source.png", "odd_distorted.png", 15.956_974_09),
    ("small_source.png", "small_distorted.png", 94.053_004_87),
];

#[test]
fn one_shot_matches_goldens() {
    let mut failures = vec![];
    for &(src, dst, expected) in CASES {
        let score = compute_frame_ssimulacra2(load_rgb(src), load_rgb(dst)).unwrap();
        if (score - expected).abs() >= 1e-4 {
            failures.push(format!("{src}: got {score:.8}, expected {expected:.8}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
