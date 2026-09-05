use std::io::Write;

use hap_resigner::hap::info::read_hap_info;
use zip::write::SimpleFileOptions;

#[test]
fn reads_bundle_version_and_main_ability() {
    let temp = tempfile::NamedTempFile::new().expect("temp HAP");
    let file = temp.reopen().unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("module.json", SimpleFileOptions::default())
        .unwrap();
    write!(
        zip,
        r#"{{"app":{{"bundleName":"com.example.test","versionName":"1.2.3"}},"module":{{"mainElement":"EntryAbility"}}}}"#
    )
    .unwrap();
    zip.finish().unwrap();

    let info = read_hap_info(temp.path()).expect("HAP info");

    assert_eq!(info.bundle_name, "com.example.test");
    assert_eq!(info.version_name.as_deref(), Some("1.2.3"));
    assert_eq!(info.main_element.as_deref(), Some("EntryAbility"));
}
