use std::io::{Result, Write};

use regex::Regex;

fn main() -> Result<()> {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_path = manifest_dir.join("proto");

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let re_features = Regex::new(r"option features.*").unwrap();
    let re_reserved = Regex::new(r"(?m)^(\s*)reserved\s+([^;]+);").unwrap();
    let re_identifier = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();

    let mut proto_files = Vec::new();
    for proto in proto_path
        .read_dir()
        .unwrap()
        .map(|entry| entry.unwrap().path())
    {
        if proto.extension().is_none_or(|ext| ext != "proto") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", proto.display());

        let file_name = proto.file_name().unwrap();
        let out_path = out_dir.join(file_name);
        let mut out_file = std::fs::File::create(&out_path).unwrap();

        let proto_content = std::fs::read_to_string(&proto).expect("Failed to read proto file");

        let modified_content = re_features
            .replace_all(&proto_content, "")
            .replace(r#"edition = "2023";"#, r#"syntax = "proto3";"#)
            .replace(r#"import "google/protobuf/go_features.proto";"#, "");

        // Proto editions require reserved names to be identifiers, while the
        // proto3 syntax expected by prost requires string literals. Quote every
        // reserved name after converting the source to proto3, including names
        // in comma-separated lists, while leaving numeric reservations intact.
        let modified_content =
            re_reserved.replace_all(&modified_content, |captures: &regex::Captures<'_>| {
                let values = captures[2]
                    .split(',')
                    .map(|value| {
                        let value = value.trim();
                        if re_identifier.is_match(value) {
                            format!("\"{value}\"")
                        } else {
                            value.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                format!("{}reserved {values};", &captures[1])
            });

        out_file
            .write_all(modified_content.as_bytes())
            .expect("Failed to write to output file");

        proto_files.push(out_path);
    }

    prost_reflect_build::Builder::new()
        .descriptor_pool("crate::DESCRIPTOR_POOL")
        .compile_protos(&proto_files, &[out_dir])?;
    Ok(())
}
