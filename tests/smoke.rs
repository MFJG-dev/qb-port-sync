use qb_port_sync::Config;

#[test]
fn config_example_deserializes() {
    let raw = std::fs::read_to_string("config/config.example.toml").expect("read example config");
    let config: Config = toml::from_str(&raw).expect("parse config example");
    assert_eq!(config.qbittorrent.base_url, "http://127.0.0.1:8080");
}
