use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ssimulacra2/test_data")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn score_with(args: &[&str]) -> f64 {
    let out = Command::new(env!("CARGO_BIN_EXE_ssimulacra2_rs"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "exit {:?}, stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("Score: "))
        .expect("no Score line")
        .trim()
        .parse()
        .unwrap()
}

// tank_distorted.png est un PNG RGB 16 bits avec un chunk iCCP : le chemin ICC
// 16 bits doit fonctionner (régression : panic lcms2 "6 bytes per pixel").
#[test]
fn icc_transform_on_16bit_png_does_not_crash() {
    let src = fixture("odd_source.png");
    let dst = fixture("odd_distorted.png");
    let with_icc = score_with(&["image", &src, &dst]);
    let without_icc = score_with(&["image", "--no-icc", &src, &dst]);
    // Le profil embarqué est sRGB : la conversion doit être quasi neutre.
    assert!(
        (with_icc - without_icc).abs() < 0.25,
        "icc {with_icc} vs no-icc {without_icc}"
    );
}
