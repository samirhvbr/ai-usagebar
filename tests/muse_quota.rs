use ai_usagebar::config::Config;
use ai_usagebar::vendor::VendorId;
use ai_usagebar::widget::cli::Cli;
use clap::Parser;

#[test]
fn native_muse_code_is_opt_in_and_does_not_collide_with_legacy_custom_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
[muse-code]
enabled = true
credentials_path = "/fixture/auth.json"
[[custom]]
id = "muse"
name = "Muse local"
short_name = "mus"
url = "http://127.0.0.1:41837/usage"
allow_http = true
[[custom.texts]]
label = "Calls"
value = "/calls/today"
"#,
    )
    .unwrap();
    let config = Config::load_from(&path).unwrap();
    assert!(config.is_enabled(VendorId::MuseCode));
    assert!(config.custom_by_id("muse").is_some());
    assert!(!Config::default().muse.enabled);
    assert!(!VendorId::MuseCode.auto_detectable());
    assert_eq!(VendorId::MuseCode.slug(), "muse-code");
    assert_eq!(VendorId::MuseCode.short_name(), "msc");
    let cli = Cli::try_parse_from(["ai-usagebar", "--vendor", "muse-code"]).unwrap();
    assert_eq!(cli.vendor.unwrap().to_id(), VendorId::MuseCode);
}
