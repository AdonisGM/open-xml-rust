//! `openxml` — inspect, round-trip and validate Office Open XML packages.
//!
//! ```text
//! openxml info <file>                  list parts, content types and relationships
//! openxml cat <file> <part>            print a part (e.g. /word/document.xml)
//! openxml roundtrip <file> [<out>]     parse every XML part into the typed model, write it back
//!                                      and compare; optionally save the re-serialized package
//! openxml validate <file> [--schemas <dir>]
//!                                      validate XML parts against the ECMA-376 schemas (xmllint)
//! ```

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use openxml_opc::{Package, PartName};
use openxml_xml::compare::{DiffKind, semantic_diff};
use openxml_xml::{Ns, RawElement, decode_xml_bytes};

const USAGE: &str = "usage:
  openxml info <file>
  openxml cat <file> <part>
  openxml roundtrip <file> [<out>]
  openxml validate <file> [--schemas <dir>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("info") if args.len() == 2 => info(&args[1]),
        Some("cat") if args.len() == 3 => cat(&args[1], &args[2]),
        Some("roundtrip") if args.len() == 2 || args.len() == 3 => roundtrip(&args[1], args.get(2)),
        Some("validate") if args.len() == 2 => validate(&args[1], None),
        Some("validate") if args.len() == 4 && args[2] == "--schemas" => validate(&args[1], Some(&args[3])),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(true)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type CliResult = Result<bool, Box<dyn std::error::Error>>;

fn info(file: &str) -> CliResult {
    let pkg = Package::open_path(file)?;
    println!("{file}");
    match pkg.main_part() {
        Some(main) => println!("main part: {main}"),
        None => println!("main part: (none)"),
    }
    let core = pkg.core_properties()?;
    if let Some(t) = &core.title {
        println!("title: {t}");
    }
    if let Some(c) = &core.creator {
        println!("creator: {c}");
    }
    println!("\n{:<48} {:>9}  content type", "part", "bytes");
    for (name, part) in pkg.parts() {
        println!(
            "{:<48} {:>9}  {}",
            name.as_str(),
            part.data().len(),
            part.content_type()
        );
    }
    println!("\nrelationships:");
    for r in pkg.package_relationships().iter() {
        println!("  / --{}--> {}", short_type(&r.rel_type), r.target);
    }
    for (name, part) in pkg.parts() {
        for r in part.relationships().iter() {
            let ext = if r.is_external() { " (external)" } else { "" };
            println!("  {name} --{}--> {}{ext}", short_type(&r.rel_type), r.target);
        }
    }
    Ok(true)
}

fn short_type(t: &str) -> &str {
    t.rsplit('/').next().unwrap_or(t)
}

fn cat(file: &str, part: &str) -> CliResult {
    let pkg = Package::open_path(file)?;
    let name = PartName::new(part)?;
    let data = pkg.part(&name).ok_or_else(|| format!("no part {part}"))?.data();
    std::io::stdout().write_all(data)?;
    Ok(true)
}

fn xml_parts(pkg: &Package) -> Vec<(PartName, String)> {
    pkg.parts()
        .filter_map(|(name, part)| {
            let text = decode_xml_bytes(part.data()).ok()?;
            text.trim_start()
                .starts_with('<')
                .then(|| (name.clone(), text.into_owned()))
        })
        .collect()
}

fn roundtrip(file: &str, out: Option<&String>) -> CliResult {
    let mut pkg = Package::open_path(file)?;
    let (mut typed, mut unknown, mut failed, mut differing) = (0, 0, 0, 0);
    for (name, text) in xml_parts(&pkg) {
        let Ok(original) = RawElement::parse(&text) else {
            continue;
        };
        match openxml_schema::round_trip_xml(&text) {
            None => unknown += 1,
            Some(Err(e)) => {
                failed += 1;
                println!("FAIL  {name}: {e}");
            }
            Some(Ok(written)) => {
                typed += 1;
                let back = RawElement::parse(&written)?;
                let diffs: Vec<_> = semantic_diff(&original, &back)
                    .into_iter()
                    .filter(|d| d.kind != DiffKind::Reordered)
                    .collect();
                if diffs.is_empty() {
                    println!("ok    {name}");
                } else {
                    differing += 1;
                    println!("DIFF  {name}");
                    for d in diffs.iter().take(10) {
                        println!("        {d}");
                    }
                }
                let ct = pkg
                    .part(&name)
                    .map(|p| p.content_type().to_owned())
                    .unwrap_or_default();
                pkg.set_part(name, &ct, written.into_bytes())?;
            }
        }
    }
    println!(
        "\n{typed} typed parts, {unknown} parts without a schema, {failed} failures, {differing} with differences"
    );
    if let Some(path) = out {
        pkg.save_path(path)?;
        println!("written {path}");
    }
    Ok(failed == 0 && differing == 0)
}

fn default_schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/validation")
}

fn validate(file: &str, schemas: Option<&String>) -> CliResult {
    let dir = schemas.map(PathBuf::from).unwrap_or_else(default_schema_dir);
    let driver = dir.join("transitional-all.xsd");
    if !driver.exists() {
        return Err(format!("schema driver not found: {}", driver.display()).into());
    }
    let pkg = Package::open_path(file)?;
    let (mut valid, mut invalid, mut skipped) = (0, 0, 0);
    for (name, text) in xml_parts(&pkg) {
        let Ok((ns, _)) = openxml_xml::root_name(&text) else {
            continue;
        };
        if !ns.is_known() || matches!(ns, Ns::CP | Ns::CT | Ns::PR) {
            skipped += 1;
            continue;
        }
        match xmllint(&driver, &text) {
            Ok(()) => {
                valid += 1;
                println!("valid    {name}");
            }
            Err(msg) => {
                invalid += 1;
                println!("INVALID  {name}");
                for line in msg.lines().filter(|l| !l.contains("fails to validate")).take(10) {
                    println!("           {}", line.trim_start_matches("-:"));
                }
            }
        }
    }
    println!("\n{valid} valid, {invalid} invalid, {skipped} not covered by the ECMA-376 schemas");
    Ok(invalid == 0)
}

fn xmllint(schema: &Path, xml: &str) -> Result<(), String> {
    let mut child = Command::new("xmllint")
        .args(["--noout", "--nonet", "--schema"])
        .arg(schema)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run xmllint: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = xml.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}
