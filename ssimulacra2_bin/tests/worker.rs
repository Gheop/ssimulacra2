use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use ssimulacra2::{ColorPrimaries, Rgb, TransferCharacteristic, compute_frame_ssimulacra2};

fn lib_rgb(w: u32, h: u32, bytes: &[u8]) -> Rgb {
    let pixels = bytes
        .chunks_exact(3)
        .map(|c| {
            [
                f32::from(c[0]) / 255.0,
                f32::from(c[1]) / 255.0,
                f32::from(c[2]) / 255.0,
            ]
        })
        .collect();
    Rgb::new(
        pixels,
        w as usize,
        h as usize,
        TransferCharacteristic::SRGB,
        ColorPrimaries::BT709,
    )
    .unwrap()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ssimulacra2/test_data")
        .join(name)
}

fn rgb8_bytes(name: &str) -> (u32, u32, Vec<u8>) {
    let img = image::open(fixture(name)).unwrap().to_rgb8();
    (img.width(), img.height(), img.into_raw())
}

fn send(stdin: &mut impl Write, tag: u8, w: u32, h: u32, fmt: u8, payload: &[u8]) {
    stdin.write_all(&[tag]).unwrap();
    stdin.write_all(&w.to_le_bytes()).unwrap();
    stdin.write_all(&h.to_le_bytes()).unwrap();
    stdin.write_all(&[fmt]).unwrap();
    stdin
        .write_all(&(payload.len() as u32).to_le_bytes())
        .unwrap();
    stdin.write_all(payload).unwrap();
    stdin.flush().unwrap();
}

#[test]
fn worker_scores_match_image_mode() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ssimulacra2_rs"))
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let (w, h, src) = rgb8_bytes("odd_source.png");
    let (_, _, dst) = rgb8_bytes("odd_distorted.png");

    send(&mut stdin, b'R', w, h, 0, &src);
    assert_eq!(lines.next().unwrap().unwrap(), "OK");

    // Trois scores identiques sur la même référence chaude.
    let mut scores = vec![];
    for _ in 0..3 {
        send(&mut stdin, b'S', w, h, 0, &dst);
        let line = lines.next().unwrap().unwrap();
        let v: f64 = line.strip_prefix("SCORE ").unwrap().parse().unwrap();
        scores.push(v);
    }
    assert_eq!(scores[0], scores[1]);
    assert_eq!(scores[1], scores[2]);

    // Même score que la lib sur les mêmes pixels 8 bits (odd_distorted.png est
    // un PNG 16 bits : le mode image le décode en 16 bits natifs, il ne peut
    // donc pas servir d'oracle pour un envoi rgb8).
    let expected = compute_frame_ssimulacra2(lib_rgb(w, h, &src), lib_rgb(w, h, &dst)).unwrap();
    assert!(
        (scores[0] - expected).abs() < 1e-6,
        "worker {} vs lib {expected}",
        scores[0]
    );

    // Dimensions incohérentes -> ERR, la session continue.
    send(
        &mut stdin,
        b'S',
        w + 2,
        h,
        0,
        &vec![0u8; ((w + 2) * h * 3) as usize],
    );
    assert!(lines.next().unwrap().unwrap().starts_with("ERR "));
    send(&mut stdin, b'S', w, h, 0, &dst);
    assert!(lines.next().unwrap().unwrap().starts_with("SCORE "));

    // EOF -> sortie propre.
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn worker_accepts_png_payload() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ssimulacra2_rs"))
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let src_png = std::fs::read(fixture("small_source.png")).unwrap();
    let (w, h, dst) = rgb8_bytes("small_distorted.png");

    // fmt=1 : payload PNG, dimensions du header ignorées au décodage.
    send(&mut stdin, b'R', 0, 0, 1, &src_png);
    assert_eq!(lines.next().unwrap().unwrap(), "OK");
    send(&mut stdin, b'S', w, h, 0, &dst);
    assert!(lines.next().unwrap().unwrap().starts_with("SCORE "));

    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn worker_accepts_rgb16_payload() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ssimulacra2_rs"))
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let (w, h, src) = rgb8_bytes("small_source.png");
    let (_, _, dst) = rgb8_bytes("small_distorted.png");

    // u8 -> u16 plein-échelle (v * 257) : mêmes valeurs normalisées, le score
    // doit être identique au chemin RGB8.
    let widen = |bytes: &[u8]| -> Vec<u8> {
        bytes
            .iter()
            .flat_map(|&v| (u16::from(v) * 257).to_le_bytes())
            .collect()
    };

    send(&mut stdin, b'R', w, h, 2, &widen(&src));
    assert_eq!(lines.next().unwrap().unwrap(), "OK");
    send(&mut stdin, b'S', w, h, 2, &widen(&dst));
    let line16 = lines.next().unwrap().unwrap();
    let s16: f64 = line16.strip_prefix("SCORE ").unwrap().parse().unwrap();

    send(&mut stdin, b'R', w, h, 0, &src);
    assert_eq!(lines.next().unwrap().unwrap(), "OK");
    send(&mut stdin, b'S', w, h, 0, &dst);
    let line8 = lines.next().unwrap().unwrap();
    let s8: f64 = line8.strip_prefix("SCORE ").unwrap().parse().unwrap();

    assert!((s16 - s8).abs() < 1e-4, "rgb16 {s16} vs rgb8 {s8}");

    // Payload de taille fausse -> ERR, la session continue.
    send(&mut stdin, b'S', w, h, 2, &vec![0u8; 10]);
    assert!(lines.next().unwrap().unwrap().starts_with("ERR "));

    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn worker_score_before_ref_is_an_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ssimulacra2_rs"))
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let (w, h, dst) = rgb8_bytes("small_distorted.png");
    send(&mut stdin, b'S', w, h, 0, &dst);
    assert!(lines.next().unwrap().unwrap().starts_with("ERR "));
    drop(stdin);
    assert!(child.wait().unwrap().success());
}
