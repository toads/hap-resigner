use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use clap::{Parser, Subcommand};
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
use thiserror::Error;

use crate::hap::format::{TYPE_SIGNER, parse_hap};
use crate::hap::sign::sign_hap;
use crate::materials::generate::generate_key_material;
use crate::materials::load_signing_identity;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("signing material failed: {0}")]
    Material(String),
    #[error("HAP signing failed: {0}")]
    Sign(String),
    #[error("selftest failed: {0}")]
    Selftest(String),
}

#[derive(Debug, Clone)]
pub struct SignOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub p12: PathBuf,
    pub certificate: PathBuf,
    pub profile: PathBuf,
    pub password: String,
}

#[derive(Debug, Parser)]
#[command(name = "hap-resigner", disable_help_subcommand = true)]
struct Args {
    #[arg(long)]
    selftest: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Sign {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        p12: PathBuf,
        #[arg(long)]
        certificate: PathBuf,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, default_value = "123456")]
        password: String,
    },
}

pub fn run_from_env() -> Result<bool, CliError> {
    let args = Args::parse();
    if args.selftest {
        selftest()?;
        println!("SELFTEST_OK");
        return Ok(true);
    }
    match args.command {
        Some(Command::Sign {
            input,
            output,
            p12,
            certificate,
            profile,
            password,
        }) => {
            sign_file(&SignOptions {
                input,
                output,
                p12,
                certificate,
                profile,
                password,
            })?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn sign_file(options: &SignOptions) -> Result<(), CliError> {
    let input = fs::read(&options.input)?;
    let p12 = fs::read(&options.p12)?;
    let certificate = fs::read(&options.certificate)?;
    let profile = fs::read(&options.profile)?;
    let identity = load_signing_identity(&p12, &options.password, &certificate)
        .map_err(|error| CliError::Material(error.to_string()))?;
    let output =
        sign_hap(&input, &identity, &profile).map_err(|error| CliError::Sign(error.to_string()))?;
    atomic_write(&options.output, &output)?;
    Ok(())
}

pub fn selftest() -> Result<(), CliError> {
    let generated = generate_key_material("selftest", "selftest-password")
        .map_err(|error| CliError::Selftest(error.to_string()))?;
    let key_store = KeyStore::from_pkcs12(
        &generated.p12,
        "selftest-password",
        Pkcs12ImportPolicy::Strict,
    )
    .map_err(|error| CliError::Selftest(error.to_string()))?;
    let (_, chain) = key_store
        .private_key_chain()
        .ok_or_else(|| CliError::Selftest("generated P12 has no private key".to_owned()))?;
    let placeholder = chain
        .certs()
        .first()
        .ok_or_else(|| CliError::Selftest("generated P12 has no certificate".to_owned()))?;
    let certificate_pem = pem::encode(&pem::Pem::new("CERTIFICATE", placeholder.as_der().to_vec()));
    let identity = load_signing_identity(
        &generated.p12,
        "selftest-password",
        certificate_pem.as_bytes(),
    )
    .map_err(|error| CliError::Selftest(error.to_string()))?;
    let unsigned_zip = empty_zip();
    let signed = sign_hap(&unsigned_zip, &identity, b"profile")
        .map_err(|error| CliError::Selftest(error.to_string()))?;
    let layout = parse_hap(&signed).map_err(|error| CliError::Selftest(error.to_string()))?;
    let signer_present = layout
        .signing_block
        .as_ref()
        .and_then(|block| block.block_value(&signed, TYPE_SIGNER))
        .is_some();
    if !signer_present {
        return Err(CliError::Selftest("signer block is missing".to_owned()));
    }
    Ok(())
}

fn empty_zip() -> Vec<u8> {
    let mut eocd = Vec::with_capacity(22);
    eocd.extend_from_slice(b"PK\x05\x06");
    eocd.extend_from_slice(&[0; 16]);
    eocd.extend_from_slice(&0_u16.to_le_bytes());
    eocd
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = AtomicWriteFile::options().open(path)?;
    file.write_all(contents)?;
    file.commit()
}
