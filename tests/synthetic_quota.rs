use ai_usagebar::config::Config;
use ai_usagebar::vendor::{AuthKind, VendorId};
use ai_usagebar::widget::cli::Cli;
use clap::Parser;

#[test]
fn synthetic_is_opt_in_and_wired_like_other_key_vendors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        "[synthetic]\nenabled = true\napi_key = \"test-key\"\n",
    )
    .unwrap();

    let config = Config::load_from(&path).unwrap();
    assert!(config.is_enabled(VendorId::Synthetic));
    assert_eq!(config.inline_api_key(VendorId::Synthetic), Some("test-key"));

    let default = Config::default();
    assert!(!default.synthetic.enabled);
    assert_eq!(
        default.api_key_env_for(VendorId::Synthetic),
        "SYNTHETIC_API_KEY"
    );
    assert_eq!(VendorId::Synthetic.auth_kind(), AuthKind::ApiKey);

    let cli = Cli::try_parse_from(["ai-usagebar", "--vendor", "synthetic"]).unwrap();
    assert_eq!(cli.vendor.unwrap().to_id(), VendorId::Synthetic);
}
