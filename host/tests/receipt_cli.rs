use anyhow::Result;
use greenhold_guest_builder::GREENHOLD_GUEST_ID;
use risc0_zkvm::sha::Digest;
use std::{fs, process::Command};

#[test]
fn version_names_the_guest_image_id_that_receipts_must_match() -> Result<()> {
    let expected = format!(
        "greenhold {}\nguest image ID: {}\n",
        env!("CARGO_PKG_VERSION"),
        Digest::from(GREENHOLD_GUEST_ID)
    );
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_greenhold"))
            .arg(flag)
            .output()?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout)?, expected);
    }
    Ok(())
}

#[test]
fn verify_and_inspect_reject_bad_receipt_files_before_node_access() -> Result<()> {
    let binary = env!("CARGO_BIN_EXE_greenhold");
    let directory = tempfile::tempdir()?;
    let tag = b"greenhold/receipt/v1\0";
    let mut garbage = tag.to_vec();
    garbage.extend([0xff; 8]);
    let mut largest = tag.to_vec();
    largest.resize(1 << 20, 0);
    let mut oversized = largest.clone();
    oversized.push(0);
    let files = [
        (None, "cannot read receipt"),
        (
            Some(b"greenhold/receipt/v2\0".to_vec()),
            "receipt tag or version is not supported",
        ),
        (Some(garbage), "receipt decoding"),
        // A file at the size limit is decoded. One more byte is rejected unread.
        (Some(largest), "receipt decoding"),
        (Some(oversized), "receipt file exceeds 1048576 bytes"),
    ];
    for (index, (contents, message)) in files.into_iter().enumerate() {
        let path = directory.path().join(format!("{index}.receipt"));
        if let Some(contents) = contents {
            fs::write(&path, contents)?;
        }
        let path = path.to_str().unwrap_or_default();
        // An unused local port: node access would fail with a connection error instead.
        let verify = [
            "verify",
            "--receipt",
            path,
            "--utreexod-rpc",
            "http://127.0.0.1:9/",
            "--utreexod-rpc-user",
            "user",
            "--utreexod-rpc-password",
            "pass",
        ];
        for args in [&verify[..], &["inspect", "--receipt", path][..]] {
            let output = Command::new(binary).args(args).output()?;
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8(output.stderr)?;
            assert!(stderr.contains(message), "{stderr}");
        }
    }
    Ok(())
}
