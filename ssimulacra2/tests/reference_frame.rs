use std::path::PathBuf;

use ssimulacra2::{
    ColorPrimaries, ReferenceFrame, Rgb, Ssimulacra2Error, TransferCharacteristic,
    compute_frame_ssimulacra2,
};

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

#[test]
fn matches_one_shot_exactly() {
    for (s, d) in [
        ("tank_source.png", "tank_distorted.png"),
        ("odd_source.png", "odd_distorted.png"),
        ("small_source.png", "small_distorted.png"),
    ] {
        let one_shot = compute_frame_ssimulacra2(load_rgb(s), load_rgb(d)).unwrap();
        let via_ref = ReferenceFrame::new(load_rgb(s))
            .unwrap()
            .score(load_rgb(d))
            .unwrap();
        assert!(
            (one_shot - via_ref).abs() < 1e-9,
            "{s}: one-shot {one_shot} != ref {via_ref}"
        );
    }
}

#[test]
fn score_is_repeatable() {
    let r = ReferenceFrame::new(load_rgb("odd_source.png")).unwrap();
    let a = r.score(load_rgb("odd_distorted.png")).unwrap();
    let b = r.score(load_rgb("odd_distorted.png")).unwrap();
    assert_eq!(a, b);
}

#[test]
fn dimension_mismatch_is_rejected() {
    let r = ReferenceFrame::new(load_rgb("tank_source.png")).unwrap();
    let err = r.score(load_rgb("odd_distorted.png")).unwrap_err();
    assert_eq!(err, Ssimulacra2Error::NonMatchingImageDimensions);
}

#[test]
fn too_small_reference_is_rejected() {
    let px = vec![[0.5f32; 3]; 4 * 4];
    let rgb = Rgb::new(
        px,
        4,
        4,
        TransferCharacteristic::SRGB,
        ColorPrimaries::BT709,
    )
    .unwrap();
    let err = ReferenceFrame::new(rgb).unwrap_err();
    assert_eq!(err, Ssimulacra2Error::InvalidImageSize);
}
