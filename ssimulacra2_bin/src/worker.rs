use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};

use anyhow::{Context, bail};
use ssimulacra2::{ColorPrimaries, ReferenceFrame, Rgb, TransferCharacteristic};

const FMT_RGB8: u8 = 0;
const FMT_PNG: u8 = 1;
const FMT_RGB16: u8 = 2;
// Garde-fou protocole : au-delà, c'est un flux corrompu, pas une image.
const MAX_PAYLOAD: u32 = 512 * 1024 * 1024;

struct Request {
    tag: u8,
    width: usize,
    height: usize,
    fmt: u8,
    payload: Vec<u8>,
}

fn read_request(input: &mut impl Read) -> anyhow::Result<Option<Request>> {
    let mut tag = [0u8; 1];
    match input.read_exact(&mut tag) {
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        r => r.context("read tag")?,
    }
    let mut u32buf = [0u8; 4];
    input.read_exact(&mut u32buf).context("read width")?;
    let width = u32::from_le_bytes(u32buf) as usize;
    input.read_exact(&mut u32buf).context("read height")?;
    let height = u32::from_le_bytes(u32buf) as usize;
    let mut fmt = [0u8; 1];
    input.read_exact(&mut fmt).context("read pixfmt")?;
    input.read_exact(&mut u32buf).context("read len")?;
    let len = u32::from_le_bytes(u32buf);
    if len > MAX_PAYLOAD {
        bail!("payload length {len} exceeds {MAX_PAYLOAD}");
    }
    let mut payload = vec![0u8; len as usize];
    input.read_exact(&mut payload).context("read payload")?;
    Ok(Some(Request {
        tag: tag[0],
        width,
        height,
        fmt: fmt[0],
        payload,
    }))
}

fn u8_to_rgb_pixels(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .chunks_exact(3)
        .map(|c| {
            [
                f32::from(c[0]) / 255.0,
                f32::from(c[1]) / 255.0,
                f32::from(c[2]) / 255.0,
            ]
        })
        .collect()
}

fn decode_rgb(req: &Request) -> Result<Rgb, String> {
    let (pixels, width, height) = match req.fmt {
        FMT_RGB8 => {
            let expected = req.width * req.height * 3;
            if req.payload.len() != expected {
                return Err(format!(
                    "payload {} bytes, expected {expected} for {}x{} rgb8",
                    req.payload.len(),
                    req.width,
                    req.height
                ));
            }
            (u8_to_rgb_pixels(&req.payload), req.width, req.height)
        }
        FMT_RGB16 => {
            let expected = req.width * req.height * 3 * 2;
            if req.payload.len() != expected {
                return Err(format!(
                    "payload {} bytes, expected {expected} for {}x{} rgb16",
                    req.payload.len(),
                    req.width,
                    req.height
                ));
            }
            let pixels = req
                .payload
                .chunks_exact(6)
                .map(|c| {
                    [
                        f32::from(u16::from_le_bytes([c[0], c[1]])) / 65535.0,
                        f32::from(u16::from_le_bytes([c[2], c[3]])) / 65535.0,
                        f32::from(u16::from_le_bytes([c[4], c[5]])) / 65535.0,
                    ]
                })
                .collect::<Vec<_>>();
            (pixels, req.width, req.height)
        }
        FMT_PNG => {
            let img = image::load_from_memory(&req.payload)
                .map_err(|e| format!("png decode: {e}"))?;
            if img.color().has_alpha() {
                return Err("alpha unsupported in worker mode".into());
            }
            let rgb = img.to_rgb8();
            let (w, h) = (rgb.width() as usize, rgb.height() as usize);
            (u8_to_rgb_pixels(&rgb.into_raw()), w, h)
        }
        other => return Err(format!("unknown pixel format {other}")),
    };
    Rgb::new(
        pixels,
        width,
        height,
        TransferCharacteristic::SRGB,
        ColorPrimaries::BT709,
    )
    .map_err(|e| format!("rgb conversion: {e:?}"))
}

/// Boucle du worker : lit des requêtes length-prefixed sur stdin, répond une
/// ligne par requête sur stdout. Conçu pour être piloté par un process parent
/// (patu) qui garde la référence chaude pendant une recherche de qualité.
pub fn run() -> anyhow::Result<()> {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut output = BufWriter::new(std::io::stdout().lock());
    let mut reference: Option<ReferenceFrame> = None;

    while let Some(req) = read_request(&mut input)? {
        let reply = match (req.tag, decode_rgb(&req)) {
            (_, Err(msg)) => format!("ERR {msg}"),
            (b'R', Ok(rgb)) => match ReferenceFrame::new(rgb) {
                Ok(r) => {
                    reference = Some(r);
                    "OK".to_string()
                }
                Err(e) => format!("ERR {e}"),
            },
            (b'S', Ok(rgb)) => match &reference {
                None => "ERR no reference set".to_string(),
                Some(r) => match r.score(rgb) {
                    Ok(score) => format!("SCORE {score:.8}"),
                    Err(e) => format!("ERR {e}"),
                },
            },
            (tag, Ok(_)) => bail!("unknown request tag {tag:#x}"),
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}
