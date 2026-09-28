//! Corrupted input must produce errors, never panics or hangs.
//!
//! Real parts are mutated deterministically (truncation, byte flips,
//! duplicated and deleted ranges) and fed to the typed readers.

use openxml_opc::Package;
use openxml_testkit::fixture;
use openxml_xml::decode_xml_bytes;

/// A tiny deterministic generator (64-bit LCG), so failures are reproducible.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(input: &[u8], rng: &mut Lcg) -> Vec<u8> {
    let mut v = input.to_vec();
    for _ in 0..1 + rng.below(4) {
        if v.is_empty() {
            break;
        }
        let i = rng.below(v.len());
        match rng.below(5) {
            0 => v.truncate(i),
            1 => {
                const BYTES: &[u8] = b"<>/=\"'&;:a \n\x00";
                v[i] = BYTES[rng.below(BYTES.len())];
            }
            2 => {
                let j = (i + rng.below(64)).min(v.len());
                let chunk = v[i..j].to_vec();
                v.splice(i..i, chunk);
            }
            3 => {
                let j = (i + rng.below(64)).min(v.len());
                v.drain(i..j);
            }
            _ => v[i] ^= 1 << rng.below(8),
        }
    }
    v
}

#[test]
fn mutated_parts_never_panic() {
    let mut rng = Lcg(0x5EED);
    let mut inputs = Vec::new();
    for name in [
        "poi/sample.docx",
        "poi/SampleSS.xlsx",
        "poi/SampleShow.pptx",
        "poi/chartex.docx",
    ] {
        let pkg = Package::open_path(fixture(name)).unwrap();
        for (_, part) in pkg.parts() {
            if part.content_type().ends_with("xml") && part.data().len() < 200_000 {
                inputs.push(part.data().to_vec());
            }
        }
    }
    assert!(inputs.len() > 20);
    let (mut ok, mut err) = (0, 0);
    for round in 0..2_000 {
        let base = &inputs[round % inputs.len()];
        let bytes = mutate(base, &mut rng);
        let Ok(text) = decode_xml_bytes(&bytes) else {
            err += 1;
            continue;
        };
        match openxml_schema::round_trip_xml(&text) {
            Some(Ok(out)) => {
                ok += 1;
                // Whatever was accepted must be written as well-formed XML.
                assert!(openxml_xml::RawElement::parse(&out).is_ok(), "round {round}");
            }
            Some(Err(_)) | None => err += 1,
        }
        let _ = openxml_schema::validate_xml(&text);
    }
    println!("{ok} mutated inputs accepted, {err} rejected");
    assert!(ok > 0 && err > 0);
}

#[test]
fn corrupted_packages_are_rejected_cleanly() {
    let original = std::fs::read(fixture("poi/sample.docx")).unwrap();
    let mut rng = Lcg(42);
    for _ in 0..300 {
        let bytes = mutate(&original, &mut rng);
        if let Ok(pkg) = Package::from_bytes(&bytes) {
            let _ = pkg.to_bytes();
        }
    }
}
